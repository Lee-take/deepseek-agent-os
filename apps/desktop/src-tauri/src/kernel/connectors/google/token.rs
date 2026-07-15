use std::io::{Cursor, Read};
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use zeroize::Zeroize;

use super::{
    normalized_scopes, validate_google_client_id, GoogleCodeExchange, GoogleCredentialEnvelope,
    GoogleRefreshedCredential, GoogleTokenRefresher, GOOGLE_TOKEN_ENDPOINT,
};
use crate::kernel::connectors::oauth::{validate_loopback_redirect_uri, ConnectorOAuthExchange};
use crate::kernel::connectors::ConnectorSecret;

const MAX_TOKEN_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_TOKEN_REQUEST_BYTES: usize = 64 * 1024;
const MAX_TOKEN_LIFETIME_SECONDS: i64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoogleTokenFailure {
    InvalidRequest,
    CredentialUnavailable,
    Transient,
    Timeout,
    Network,
    ResponseTooLarge,
    InvalidResponse,
}

impl GoogleTokenFailure {
    pub(super) fn message(self) -> String {
        match self {
            Self::CredentialUnavailable => "Google credential is unavailable",
            Self::Transient => "Google token service is temporarily unavailable",
            Self::Timeout => "Google token request timed out",
            Self::Network => "Google token service could not be reached",
            Self::ResponseTooLarge | Self::InvalidResponse => "Google token response was invalid",
            Self::InvalidRequest => "Google token request was invalid",
        }
        .to_string()
    }
}

pub struct GoogleOAuthTokenClient {
    client_id: String,
    endpoint: reqwest::Url,
    client: reqwest::blocking::Client,
}

impl GoogleOAuthTokenClient {
    pub fn new(client_id: String) -> Result<Self, GoogleTokenFailure> {
        let endpoint = reqwest::Url::parse(GOOGLE_TOKEN_ENDPOINT)
            .map_err(|_| GoogleTokenFailure::InvalidRequest)?;
        Self::build(client_id, endpoint, true)
    }

    fn build(
        client_id: String,
        endpoint: reqwest::Url,
        https_only: bool,
    ) -> Result<Self, GoogleTokenFailure> {
        validate_google_client_id(&client_id).map_err(|_| GoogleTokenFailure::InvalidRequest)?;
        if !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(GoogleTokenFailure::InvalidRequest);
        }
        let client = reqwest::blocking::Client::builder()
            .user_agent("DS-Agent-Connector/1.0")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .https_only(https_only)
            .build()
            .map_err(|_| GoogleTokenFailure::InvalidRequest)?;
        Ok(Self {
            client_id,
            endpoint,
            client,
        })
    }

    #[cfg(test)]
    pub(super) fn new_test_http(client_id: String, port: u16) -> Result<Self, GoogleTokenFailure> {
        let endpoint = reqwest::Url::parse(&format!("http://127.0.0.1:{port}/token"))
            .map_err(|_| GoogleTokenFailure::InvalidRequest)?;
        Self::build(client_id, endpoint, false)
    }

    fn request_token(
        &self,
        fields: &[(&str, &str)],
        fallback_scopes: &[String],
    ) -> Result<GoogleTokenPayload, GoogleTokenFailure> {
        let mut form = encode_form(fields)?;
        let form_len = form.len();
        let reader = ZeroizingReader(Cursor::new(std::mem::take(&mut form)));
        let mut response = self
            .client
            .post(self.endpoint.clone())
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(reqwest::blocking::Body::sized(reader, form_len as u64))
            .send()
            .map_err(|error| {
                if error.is_timeout() {
                    GoogleTokenFailure::Timeout
                } else {
                    GoogleTokenFailure::Network
                }
            })?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_TOKEN_RESPONSE_BYTES as u64)
        {
            return Err(GoogleTokenFailure::ResponseTooLarge);
        }
        let status = response.status().as_u16();
        let mut body = Vec::with_capacity(MAX_TOKEN_RESPONSE_BYTES.min(8192));
        if let Err(error) = response
            .by_ref()
            .take(MAX_TOKEN_RESPONSE_BYTES as u64 + 1)
            .read_to_end(&mut body)
        {
            body.zeroize();
            return Err(if error.kind() == std::io::ErrorKind::TimedOut {
                GoogleTokenFailure::Timeout
            } else {
                GoogleTokenFailure::Network
            });
        }
        if body.len() > MAX_TOKEN_RESPONSE_BYTES {
            body.zeroize();
            return Err(GoogleTokenFailure::ResponseTooLarge);
        }
        if status != 200 {
            let invalid_grant = serde_json::from_slice::<GoogleTokenError>(&body)
                .ok()
                .is_some_and(|error| error.error.as_deref() == Some("invalid_grant"));
            body.zeroize();
            return Err(if status == 401 || status == 403 || invalid_grant {
                GoogleTokenFailure::CredentialUnavailable
            } else if status == 429 || status >= 500 {
                GoogleTokenFailure::Transient
            } else {
                GoogleTokenFailure::InvalidResponse
            });
        }
        let raw = serde_json::from_slice::<GoogleTokenResponse>(&body)
            .map_err(|_| GoogleTokenFailure::InvalidResponse);
        body.zeroize();
        raw?.into_payload(fallback_scopes)
    }
}

impl GoogleCodeExchange for GoogleOAuthTokenClient {
    fn exchange_code(
        &self,
        code: &str,
        verifier: &ConnectorSecret,
        redirect_uri: &str,
        requested_scopes: &[String],
    ) -> Result<ConnectorOAuthExchange, String> {
        let valid_verifier = (43..=128).contains(&verifier.expose().len())
            && verifier.expose().bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
            });
        if code.trim().is_empty()
            || code.len() > 16 * 1024
            || !valid_verifier
            || requested_scopes.is_empty()
            || requested_scopes.len() > 32
            || requested_scopes
                .iter()
                .any(|scope| scope.trim().is_empty() || scope.len() > 256)
            || validate_loopback_redirect_uri(redirect_uri).is_err()
        {
            return Err(GoogleTokenFailure::InvalidRequest.message());
        }
        let payload = self
            .request_token(
                &[
                    ("client_id", &self.client_id),
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("redirect_uri", redirect_uri),
                    ("code_verifier", verifier.expose()),
                ],
                requested_scopes,
            )
            .map_err(GoogleTokenFailure::message)?;
        if normalized_scopes(requested_scopes) != normalized_scopes(&payload.access_scopes) {
            return Err("OAuth token scopes did not match the approved request".to_string());
        }
        let refresh_token = payload
            .refresh_token
            .as_ref()
            .ok_or_else(|| "Google token response was invalid".to_string())?;
        let envelope = GoogleCredentialEnvelope::new(
            &payload.access_token,
            refresh_token,
            payload.expires_at,
            payload.access_scopes.clone(),
        );
        ConnectorOAuthExchange::new(envelope.encode()?, payload.access_scopes)
    }
}

impl GoogleTokenRefresher for GoogleOAuthTokenClient {
    fn refresh(
        &self,
        refresh_token: &ConnectorSecret,
        access_scopes: &[String],
    ) -> Result<GoogleRefreshedCredential, GoogleTokenFailure> {
        if refresh_token.expose().len() > 48 * 1024
            || access_scopes.is_empty()
            || access_scopes.len() > 32
        {
            return Err(GoogleTokenFailure::InvalidRequest);
        }
        let payload = self.request_token(
            &[
                ("client_id", &self.client_id),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.expose()),
            ],
            access_scopes,
        )?;
        if normalized_scopes(access_scopes) != normalized_scopes(&payload.access_scopes) {
            return Err(GoogleTokenFailure::InvalidResponse);
        }
        GoogleRefreshedCredential::new(
            payload.access_token,
            payload.refresh_token,
            payload.expires_at,
            payload.access_scopes,
        )
        .map_err(|_| GoogleTokenFailure::InvalidResponse)
    }
}

struct GoogleTokenPayload {
    access_token: ConnectorSecret,
    refresh_token: Option<ConnectorSecret>,
    expires_at: chrono::DateTime<Utc>,
    access_scopes: Vec<String>,
}

#[derive(Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
    token_type: String,
    scope: Option<String>,
}

impl GoogleTokenResponse {
    fn into_payload(
        mut self,
        fallback_scopes: &[String],
    ) -> Result<GoogleTokenPayload, GoogleTokenFailure> {
        if !self.token_type.eq_ignore_ascii_case("bearer")
            || self.expires_in <= 0
            || self.expires_in > MAX_TOKEN_LIFETIME_SECONDS
        {
            return Err(GoogleTokenFailure::InvalidResponse);
        }
        let access_scopes = self
            .scope
            .as_deref()
            .map(|scope| {
                scope
                    .split_ascii_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|scopes| !scopes.is_empty())
            .unwrap_or_else(|| normalized_scopes(fallback_scopes));
        if access_scopes.is_empty() {
            return Err(GoogleTokenFailure::InvalidResponse);
        }
        let expires_at = Utc::now()
            .checked_add_signed(ChronoDuration::seconds(self.expires_in))
            .ok_or(GoogleTokenFailure::InvalidResponse)?;
        let access_token = ConnectorSecret::new(std::mem::take(&mut self.access_token))
            .map_err(|_| GoogleTokenFailure::InvalidResponse)?;
        let refresh_token = self
            .refresh_token
            .take()
            .map(ConnectorSecret::new)
            .transpose()
            .map_err(|_| GoogleTokenFailure::InvalidResponse)?;
        Ok(GoogleTokenPayload {
            access_token,
            refresh_token,
            expires_at,
            access_scopes,
        })
    }
}

impl Drop for GoogleTokenResponse {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(refresh_token) = self.refresh_token.as_mut() {
            refresh_token.zeroize();
        }
        if let Some(scope) = self.scope.as_mut() {
            scope.zeroize();
        }
    }
}

#[derive(Deserialize)]
struct GoogleTokenError {
    error: Option<String>,
}

impl Drop for GoogleTokenError {
    fn drop(&mut self) {
        if let Some(error) = self.error.as_mut() {
            error.zeroize();
        }
    }
}

struct ZeroizingReader(Cursor<Vec<u8>>);

impl Read for ZeroizingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Drop for ZeroizingReader {
    fn drop(&mut self) {
        self.0.get_mut().zeroize();
    }
}

fn encode_form(fields: &[(&str, &str)]) -> Result<Vec<u8>, GoogleTokenFailure> {
    let mut encoded = Vec::new();
    for (index, (name, value)) in fields.iter().enumerate() {
        if index > 0 {
            push_form_byte(&mut encoded, b'&')?;
        }
        encode_component(name.as_bytes(), &mut encoded)?;
        push_form_byte(&mut encoded, b'=')?;
        encode_component(value.as_bytes(), &mut encoded)?;
    }
    Ok(encoded)
}

fn encode_component(input: &[u8], output: &mut Vec<u8>) -> Result<(), GoogleTokenFailure> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in input {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                push_form_byte(output, *byte)?
            }
            b' ' => push_form_byte(output, b'+')?,
            _ => {
                push_form_byte(output, b'%')?;
                push_form_byte(output, HEX[(byte >> 4) as usize])?;
                push_form_byte(output, HEX[(byte & 0x0f) as usize])?;
            }
        }
    }
    Ok(())
}

fn push_form_byte(output: &mut Vec<u8>, byte: u8) -> Result<(), GoogleTokenFailure> {
    if output.len() == MAX_TOKEN_REQUEST_BYTES {
        output.zeroize();
        return Err(GoogleTokenFailure::InvalidRequest);
    }
    output.push(byte);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use super::*;

    const CLIENT_ID: &str = "1234567890-example.apps.googleusercontent.com";

    fn serve_once(
        status: &str,
        response_body: Vec<u8>,
    ) -> (u16, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test token listener binds");
        let port = listener.local_addr().unwrap().port();
        let (sender, receiver) = mpsc::channel();
        let status = status.to_string();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let header_end = loop {
                let mut buffer = [0u8; 4096];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
                if let Some(index) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                })
                .unwrap();
            while request.len() - header_end < content_length {
                let mut buffer = [0u8; 4096];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            sender
                .send(String::from_utf8_lossy(&request).to_string())
                .unwrap();
            let headers = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_body.len()
            );
            stream.write_all(headers.as_bytes()).unwrap();
            stream.write_all(&response_body).unwrap();
        });
        (port, receiver, handle)
    }

    #[test]
    fn google_code_exchange_posts_public_client_pkce_without_client_secret() {
        let response = br#"{
            "access_token":"google-access-marker",
            "refresh_token":"google-refresh-marker",
            "expires_in":3600,
            "token_type":"Bearer",
            "scope":"openid email https://www.googleapis.com/auth/gmail.readonly"
        }"#
        .to_vec();
        let (port, captured, server) = serve_once("200 OK", response);
        let client = GoogleOAuthTokenClient::new_test_http(CLIENT_ID.to_string(), port).unwrap();
        let requested = vec![
            "email".to_string(),
            "https://www.googleapis.com/auth/gmail.readonly".to_string(),
            "openid".to_string(),
        ];
        let exchange = client
            .exchange_code(
                "code+marker",
                &ConnectorSecret::new("v".repeat(43)).unwrap(),
                "http://127.0.0.1:43821/callback",
                &requested,
            )
            .unwrap();
        let envelope = GoogleCredentialEnvelope::decode(&exchange.credential).unwrap();
        assert_eq!(envelope.access_scopes, requested);
        let request = captured.recv().unwrap();
        server.join().unwrap();
        assert!(request.starts_with("POST /token HTTP/1.1\r\n"));
        assert!(request.contains("grant_type=authorization_code"));
        assert!(request.contains("code=code%2Bmarker"));
        assert!(request.contains(&format!("code_verifier={}", "v".repeat(43))));
        assert!(!request.contains("client_secret"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
    }

    #[test]
    fn google_refresh_preserves_approved_scopes_when_response_omits_scope() {
        let response = br#"{
            "access_token":"google-access-new",
            "expires_in":3600,
            "token_type":"Bearer"
        }"#
        .to_vec();
        let (port, captured, server) = serve_once("200 OK", response);
        let client = GoogleOAuthTokenClient::new_test_http(CLIENT_ID.to_string(), port).unwrap();
        let scopes = vec!["openid".to_string(), "email".to_string()];
        let refreshed = client
            .refresh(
                &ConnectorSecret::new("refresh+marker".to_string()).unwrap(),
                &scopes,
            )
            .unwrap();
        assert_eq!(
            normalized_scopes(&refreshed.access_scopes),
            normalized_scopes(&scopes)
        );
        assert!(refreshed.refresh_token.is_none());
        let request = captured.recv().unwrap();
        server.join().unwrap();
        assert!(request.contains("grant_type=refresh_token"));
        assert!(request.contains("refresh_token=refresh%2Bmarker"));
        assert!(!request.contains("scope="));
    }
}
