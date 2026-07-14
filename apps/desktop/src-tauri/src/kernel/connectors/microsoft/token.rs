use std::io::{Cursor, Read};
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use uuid::Uuid;
use zeroize::Zeroize;

use super::{
    validate_microsoft_access_scopes, MicrosoftCodeExchange, MicrosoftCredentialEnvelope,
    MicrosoftRefreshedCredential, MicrosoftTokenRefresher, MICROSOFT_AUTHORITY,
};
use crate::kernel::connectors::oauth::ConnectorOAuthExchange;
use crate::kernel::connectors::ConnectorSecret;

const MAX_TOKEN_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_TOKEN_REQUEST_BYTES: usize = 64 * 1024;
const MAX_TOKEN_LIFETIME_SECONDS: i64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MicrosoftTokenFailure {
    InvalidRequest,
    CredentialUnavailable,
    Transient,
    Timeout,
    Network,
    ResponseTooLarge,
    InvalidResponse,
}

impl MicrosoftTokenFailure {
    fn message(self) -> String {
        match self {
            Self::CredentialUnavailable => "Microsoft credential is unavailable",
            Self::Transient => "Microsoft token service is temporarily unavailable",
            Self::Timeout => "Microsoft token request timed out",
            Self::Network => "Microsoft token service could not be reached",
            Self::ResponseTooLarge | Self::InvalidResponse => {
                "Microsoft token response was invalid"
            }
            Self::InvalidRequest => "Microsoft token request was invalid",
        }
        .to_string()
    }
}

pub struct MicrosoftOAuthTokenClient {
    client_id: String,
    endpoint: reqwest::Url,
    client: reqwest::blocking::Client,
}

impl MicrosoftOAuthTokenClient {
    pub fn new(client_id: String) -> Result<Self, MicrosoftTokenFailure> {
        let endpoint = reqwest::Url::parse(&format!("{MICROSOFT_AUTHORITY}/token"))
            .map_err(|_| MicrosoftTokenFailure::InvalidRequest)?;
        Self::build(client_id, endpoint, true)
    }

    fn build(
        client_id: String,
        endpoint: reqwest::Url,
        https_only: bool,
    ) -> Result<Self, MicrosoftTokenFailure> {
        Uuid::parse_str(&client_id).map_err(|_| MicrosoftTokenFailure::InvalidRequest)?;
        if endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.query().is_none()
            && endpoint.fragment().is_none()
        {
            // validated below
        } else {
            return Err(MicrosoftTokenFailure::InvalidRequest);
        }
        let client = reqwest::blocking::Client::builder()
            .user_agent("DS-Agent-Connector/1.0")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .https_only(https_only)
            .build()
            .map_err(|_| MicrosoftTokenFailure::InvalidRequest)?;
        Ok(Self {
            client_id,
            endpoint,
            client,
        })
    }

    #[cfg(test)]
    pub(super) fn new_test_http(
        client_id: String,
        port: u16,
    ) -> Result<Self, MicrosoftTokenFailure> {
        let endpoint = reqwest::Url::parse(&format!(
            "http://127.0.0.1:{port}/organizations/oauth2/v2.0/token"
        ))
        .map_err(|_| MicrosoftTokenFailure::InvalidRequest)?;
        Self::build(client_id, endpoint, false)
    }

    fn request_token(
        &self,
        fields: &[(&str, &str)],
    ) -> Result<MicrosoftTokenPayload, MicrosoftTokenFailure> {
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
                    MicrosoftTokenFailure::Timeout
                } else {
                    MicrosoftTokenFailure::Network
                }
            })?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_TOKEN_RESPONSE_BYTES as u64)
        {
            return Err(MicrosoftTokenFailure::ResponseTooLarge);
        }
        let status = response.status().as_u16();
        let mut body = Vec::with_capacity(MAX_TOKEN_RESPONSE_BYTES.min(8192));
        let read = response
            .by_ref()
            .take(MAX_TOKEN_RESPONSE_BYTES as u64 + 1)
            .read_to_end(&mut body);
        if let Err(error) = read {
            body.zeroize();
            return Err(if error.kind() == std::io::ErrorKind::TimedOut {
                MicrosoftTokenFailure::Timeout
            } else {
                MicrosoftTokenFailure::Network
            });
        }
        if body.len() > MAX_TOKEN_RESPONSE_BYTES {
            body.zeroize();
            return Err(MicrosoftTokenFailure::ResponseTooLarge);
        }
        if status != 200 {
            let invalid_grant = serde_json::from_slice::<MicrosoftTokenError>(&body)
                .ok()
                .is_some_and(|error| error.is_invalid_grant());
            body.zeroize();
            return Err(if status == 401 || status == 403 || invalid_grant {
                MicrosoftTokenFailure::CredentialUnavailable
            } else if status == 429 || status >= 500 {
                MicrosoftTokenFailure::Transient
            } else {
                MicrosoftTokenFailure::InvalidResponse
            });
        }
        let raw = serde_json::from_slice::<MicrosoftTokenResponse>(&body)
            .map_err(|_| MicrosoftTokenFailure::InvalidResponse);
        body.zeroize();
        let raw = raw?;
        raw.into_payload()
    }
}

impl MicrosoftCodeExchange for MicrosoftOAuthTokenClient {
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
        let valid_scopes = !requested_scopes.is_empty()
            && requested_scopes.len() <= 32
            && requested_scopes
                .iter()
                .all(|scope| !scope.trim().is_empty() && scope.len() <= 256);
        if code.trim().is_empty()
            || code.len() > 16 * 1024
            || !valid_verifier
            || !valid_scopes
            || super::validate_loopback_redirect_uri(redirect_uri).is_err()
        {
            return Err(MicrosoftTokenFailure::InvalidRequest.message());
        }
        let scopes = requested_scopes.join(" ");
        let payload = self
            .request_token(&[
                ("client_id", &self.client_id),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier.expose()),
                ("scope", &scopes),
            ])
            .map_err(MicrosoftTokenFailure::message)?;
        validate_microsoft_access_scopes(requested_scopes, &payload.access_scopes)?;
        let refresh_token = payload
            .refresh_token
            .as_ref()
            .ok_or_else(|| "Microsoft token response was invalid".to_string())?;
        let envelope = MicrosoftCredentialEnvelope::new(
            &payload.access_token,
            refresh_token,
            payload.expires_at,
            payload.access_scopes.clone(),
        );
        ConnectorOAuthExchange::new(envelope.encode()?, payload.access_scopes)
    }
}

impl MicrosoftTokenRefresher for MicrosoftOAuthTokenClient {
    fn refresh(
        &self,
        refresh_token: &ConnectorSecret,
        access_scopes: &[String],
    ) -> Result<MicrosoftRefreshedCredential, MicrosoftTokenFailure> {
        if refresh_token.expose().len() > 48 * 1024 || access_scopes.len() > 32 {
            return Err(MicrosoftTokenFailure::InvalidRequest);
        }
        let payload = self.request_token(&[
            ("client_id", &self.client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose()),
        ])?;
        validate_microsoft_access_scopes(access_scopes, &payload.access_scopes)
            .map_err(|_| MicrosoftTokenFailure::InvalidResponse)?;
        MicrosoftRefreshedCredential::new(
            payload.access_token,
            payload.refresh_token,
            payload.expires_at,
            payload.access_scopes,
        )
        .map_err(|_| MicrosoftTokenFailure::InvalidResponse)
    }
}

struct MicrosoftTokenPayload {
    access_token: ConnectorSecret,
    refresh_token: Option<ConnectorSecret>,
    expires_at: chrono::DateTime<Utc>,
    access_scopes: Vec<String>,
}

#[derive(Deserialize)]
struct MicrosoftTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
    token_type: String,
    scope: String,
}

impl MicrosoftTokenResponse {
    fn into_payload(mut self) -> Result<MicrosoftTokenPayload, MicrosoftTokenFailure> {
        if !self.token_type.eq_ignore_ascii_case("bearer")
            || self.expires_in <= 0
            || self.expires_in > MAX_TOKEN_LIFETIME_SECONDS
        {
            return Err(MicrosoftTokenFailure::InvalidResponse);
        }
        let access_scopes = self
            .scope
            .split_ascii_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if access_scopes.is_empty() {
            return Err(MicrosoftTokenFailure::InvalidResponse);
        }
        let expires_at = Utc::now()
            .checked_add_signed(ChronoDuration::seconds(self.expires_in))
            .ok_or(MicrosoftTokenFailure::InvalidResponse)?;
        let access_token = ConnectorSecret::new(std::mem::take(&mut self.access_token))
            .map_err(|_| MicrosoftTokenFailure::InvalidResponse)?;
        let refresh_token = self
            .refresh_token
            .take()
            .map(ConnectorSecret::new)
            .transpose()
            .map_err(|_| MicrosoftTokenFailure::InvalidResponse)?;
        Ok(MicrosoftTokenPayload {
            access_token,
            refresh_token,
            expires_at,
            access_scopes,
        })
    }
}

impl Drop for MicrosoftTokenResponse {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(refresh_token) = self.refresh_token.as_mut() {
            refresh_token.zeroize();
        }
        self.scope.zeroize();
    }
}

#[derive(Deserialize)]
struct MicrosoftTokenError {
    error: Option<String>,
}

impl MicrosoftTokenError {
    fn is_invalid_grant(&self) -> bool {
        self.error.as_deref() == Some("invalid_grant")
    }
}

impl Drop for MicrosoftTokenError {
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

fn encode_form(fields: &[(&str, &str)]) -> Result<Vec<u8>, MicrosoftTokenFailure> {
    let mut encoded = Vec::new();
    for (index, (name, value)) in fields.iter().enumerate() {
        if index > 0 {
            push_form_byte(&mut encoded, b'&')?;
        }
        encode_form_component(name.as_bytes(), &mut encoded)?;
        push_form_byte(&mut encoded, b'=')?;
        encode_form_component(value.as_bytes(), &mut encoded)?;
    }
    Ok(encoded)
}

fn encode_form_component(input: &[u8], output: &mut Vec<u8>) -> Result<(), MicrosoftTokenFailure> {
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

fn push_form_byte(output: &mut Vec<u8>, byte: u8) -> Result<(), MicrosoftTokenFailure> {
    if output.len() == MAX_TOKEN_REQUEST_BYTES {
        output.zeroize();
        return Err(MicrosoftTokenFailure::InvalidRequest);
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

    const CLIENT_ID: &str = "11111111-2222-4333-8444-555555555555";

    fn serve_once(
        status: &str,
        response_headers: &[(&str, String)],
        response_body: Vec<u8>,
    ) -> (u16, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test token listener binds");
        let port = listener.local_addr().expect("test address reads").port();
        let (sender, receiver) = mpsc::channel();
        let status = status.to_string();
        let response_headers = response_headers
            .iter()
            .map(|(name, value)| ((*name).to_string(), value.clone()))
            .collect::<Vec<_>>();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("test token request accepts");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("test timeout sets");
            let mut request = Vec::new();
            let header_end = loop {
                let mut buffer = [0u8; 4096];
                let count = stream.read(&mut buffer).expect("test request reads");
                assert!(count > 0, "request ended before headers");
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
                .expect("request content length exists");
            while request.len() - header_end < content_length {
                let mut buffer = [0u8; 4096];
                let count = stream.read(&mut buffer).expect("test request body reads");
                assert!(count > 0, "request ended before body");
                request.extend_from_slice(&buffer[..count]);
            }
            sender
                .send(String::from_utf8_lossy(&request).to_string())
                .expect("captured request sends");
            let mut response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
                response_body.len()
            );
            for (name, value) in response_headers {
                response.push_str(&format!("{name}: {value}\r\n"));
            }
            response.push_str("\r\n");
            let _ = stream
                .write_all(response.as_bytes())
                .and_then(|_| stream.write_all(&response_body));
        });
        (port, receiver, handle)
    }

    #[test]
    fn form_encoding_is_deterministic_and_escapes_secrets() {
        assert_eq!(
            String::from_utf8(encode_form(&[("code", "a+b c/=")]).unwrap()).unwrap(),
            "code=a%2Bb+c%2F%3D"
        );
    }

    #[test]
    fn code_exchange_posts_exact_pkce_form_and_requires_refresh_token() {
        let response = br#"{
            "access_token":"access-marker",
            "refresh_token":"refresh-marker",
            "expires_in":3600,
            "token_type":"Bearer",
            "scope":"User.Read Mail.Read"
        }"#
        .to_vec();
        let (port, captured, server) = serve_once("200 OK", &[], response);
        let client = MicrosoftOAuthTokenClient::new_test_http(CLIENT_ID.to_string(), port)
            .expect("test token client builds");
        let requested = vec![
            "Mail.Read".to_string(),
            "User.Read".to_string(),
            "offline_access".to_string(),
        ];
        let verifier = ConnectorSecret::new("v".repeat(43)).unwrap();
        let exchange = client
            .exchange_code(
                "code+marker",
                &verifier,
                "http://127.0.0.1:43821/callback",
                &requested,
            )
            .expect("code exchange succeeds");
        let envelope = MicrosoftCredentialEnvelope::decode(&exchange.credential)
            .expect("stored credential decodes");
        assert_eq!(envelope.access_scopes, ["Mail.Read", "User.Read"]);
        let request = captured.recv().expect("request captured");
        server.join().expect("test server finishes");
        assert!(request.starts_with("POST /organizations/oauth2/v2.0/token HTTP/1.1\r\n"));
        assert!(request
            .to_ascii_lowercase()
            .contains("content-type: application/x-www-form-urlencoded"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
        assert!(!request.contains("client_secret"));
        assert!(request.contains("grant_type=authorization_code"));
        assert!(request.contains("code=code%2Bmarker"));
        assert!(request.contains(&format!("code_verifier={}", "v".repeat(43))));
        assert!(request.contains("scope=Mail.Read+User.Read+offline_access"));
    }

    #[test]
    fn refresh_keeps_rotation_optional_and_does_not_request_new_scopes() {
        let response = br#"{
            "access_token":"access-new",
            "expires_in":3600,
            "token_type":"Bearer",
            "scope":"Mail.Read User.Read"
        }"#
        .to_vec();
        let (port, captured, server) = serve_once("200 OK", &[], response);
        let client = MicrosoftOAuthTokenClient::new_test_http(CLIENT_ID.to_string(), port)
            .expect("test token client builds");
        let refreshed = client
            .refresh(
                &ConnectorSecret::new("refresh+old".to_string()).unwrap(),
                &["Mail.Read".to_string(), "User.Read".to_string()],
            )
            .expect("refresh succeeds");
        assert!(refreshed.refresh_token.is_none());
        let request = captured.recv().expect("request captured");
        server.join().expect("test server finishes");
        assert!(request.contains("grant_type=refresh_token"));
        assert!(request.contains("refresh_token=refresh%2Bold"));
        assert!(!request.contains("scope="));
        assert!(!request.contains("client_secret"));
    }

    #[test]
    fn token_client_rejects_oversized_and_invalid_grant_without_leaking_body() {
        let oversized = vec![b'x'; MAX_TOKEN_RESPONSE_BYTES + 1];
        let headers = [("Content-Type", "application/json".to_string())];
        let (port, _captured, server) = serve_once("200 OK", &headers, oversized);
        let client = MicrosoftOAuthTokenClient::new_test_http(CLIENT_ID.to_string(), port)
            .expect("test token client builds");
        let error = client
            .refresh(
                &ConnectorSecret::new("secret-marker".to_string()).unwrap(),
                &["User.Read".to_string()],
            )
            .err()
            .expect("oversized response fails");
        assert_eq!(error, MicrosoftTokenFailure::ResponseTooLarge);
        server.join().expect("test server finishes");

        let body =
            br#"{"error":"invalid_grant","error_description":"secret-provider-body"}"#.to_vec();
        let (port, _captured, server) = serve_once("400 Bad Request", &headers, body);
        let client = MicrosoftOAuthTokenClient::new_test_http(CLIENT_ID.to_string(), port)
            .expect("test token client builds");
        let error = client
            .refresh(
                &ConnectorSecret::new("secret-marker".to_string()).unwrap(),
                &["User.Read".to_string()],
            )
            .err()
            .expect("invalid grant fails");
        assert_eq!(error, MicrosoftTokenFailure::CredentialUnavailable);
        server.join().expect("test server finishes");
    }
}
