use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroize;

use super::{ConnectorAccount, ConnectorCredentialHandle, ConnectorSecret};

const MAX_CONNECTOR_HTTP_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectorHttpMethod {
    Get,
}

pub struct ConnectorHttpRequest {
    pub method: ConnectorHttpMethod,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
    pub max_response_bytes: usize,
}

impl Drop for ConnectorHttpRequest {
    fn drop(&mut self) {
        self.url.zeroize();
    }
}

pub struct ConnectorHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Drop for ConnectorHttpResponse {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

pub struct ConnectorHttpAuthContext {
    account_id: Uuid,
    credential_handle: ConnectorCredentialHandle,
}

impl ConnectorHttpAuthContext {
    pub fn for_account(account: &ConnectorAccount) -> Self {
        Self {
            account_id: account.id,
            credential_handle: account.credential_handle.clone(),
        }
    }

    pub fn account_id(&self) -> Uuid {
        self.account_id
    }

    pub fn credential_handle(&self) -> &ConnectorCredentialHandle {
        &self.credential_handle
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectorHttpFailure {
    BeforeSend,
    CredentialUnavailable,
    InvalidRequest,
    ResponseTooLarge,
    Timeout,
    Network,
}

pub trait ConnectorAccessTokenResolver: Send + Sync {
    fn resolve(
        &self,
        auth: &ConnectorHttpAuthContext,
    ) -> Result<ConnectorSecret, ConnectorHttpFailure>;
}

impl<T: ConnectorAccessTokenResolver + ?Sized> ConnectorAccessTokenResolver for std::sync::Arc<T> {
    fn resolve(
        &self,
        auth: &ConnectorHttpAuthContext,
    ) -> Result<ConnectorSecret, ConnectorHttpFailure> {
        (**self).resolve(auth)
    }
}

pub trait ConnectorHttpTransport: Send + Sync {
    fn execute(
        &self,
        auth: ConnectorHttpAuthContext,
        request: ConnectorHttpRequest,
    ) -> Result<ConnectorHttpResponse, ConnectorHttpFailure>;
}

impl<T: ConnectorHttpTransport + ?Sized> ConnectorHttpTransport for std::sync::Arc<T> {
    fn execute(
        &self,
        auth: ConnectorHttpAuthContext,
        request: ConnectorHttpRequest,
    ) -> Result<ConnectorHttpResponse, ConnectorHttpFailure> {
        (**self).execute(auth, request)
    }
}

struct ConnectorHttpOrigin {
    scheme: &'static str,
    host: &'static str,
    port: Option<u16>,
}

pub struct ReqwestConnectorHttpTransport<R> {
    client: reqwest::blocking::Client,
    access_tokens: R,
    origin: ConnectorHttpOrigin,
}

impl<R: ConnectorAccessTokenResolver> ReqwestConnectorHttpTransport<R> {
    pub fn new_microsoft(access_tokens: R) -> Result<Self, ConnectorHttpFailure> {
        Self::build(
            access_tokens,
            ConnectorHttpOrigin {
                scheme: "https",
                host: "graph.microsoft.com",
                port: None,
            },
            true,
        )
    }

    fn build(
        access_tokens: R,
        origin: ConnectorHttpOrigin,
        https_only: bool,
    ) -> Result<Self, ConnectorHttpFailure> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("DS-Agent-Connector/1.0")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .https_only(https_only)
            .build()
            .map_err(|_| ConnectorHttpFailure::BeforeSend)?;
        Ok(Self {
            client,
            access_tokens,
            origin,
        })
    }

    #[cfg(test)]
    fn new_test_http(access_tokens: R, port: u16) -> Result<Self, ConnectorHttpFailure> {
        Self::build(
            access_tokens,
            ConnectorHttpOrigin {
                scheme: "http",
                host: "127.0.0.1",
                port: Some(port),
            },
            false,
        )
    }

    fn validate_request(
        &self,
        request: &ConnectorHttpRequest,
    ) -> Result<reqwest::Url, ConnectorHttpFailure> {
        if request.method != ConnectorHttpMethod::Get
            || request.body.is_some()
            || request.max_response_bytes == 0
            || request.max_response_bytes > MAX_CONNECTOR_HTTP_RESPONSE_BYTES
        {
            return Err(ConnectorHttpFailure::InvalidRequest);
        }
        let url =
            reqwest::Url::parse(&request.url).map_err(|_| ConnectorHttpFailure::InvalidRequest)?;
        let expected_port = self.origin.port;
        let valid_origin = url.scheme() == self.origin.scheme
            && url.host_str() == Some(self.origin.host)
            && match expected_port {
                Some(port) => url.port() == Some(port),
                None => url.port().is_none(),
            }
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none();
        if !valid_origin {
            return Err(ConnectorHttpFailure::InvalidRequest);
        }
        for (name, value) in &request.headers {
            if !matches!(
                name.to_ascii_lowercase().as_str(),
                "accept" | "consistencylevel" | "prefer"
            ) || reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err()
                || reqwest::header::HeaderValue::from_str(value).is_err()
            {
                return Err(ConnectorHttpFailure::InvalidRequest);
            }
        }
        Ok(url)
    }
}

impl<R: ConnectorAccessTokenResolver> ConnectorHttpTransport for ReqwestConnectorHttpTransport<R> {
    fn execute(
        &self,
        auth: ConnectorHttpAuthContext,
        request: ConnectorHttpRequest,
    ) -> Result<ConnectorHttpResponse, ConnectorHttpFailure> {
        let url = self.validate_request(&request)?;
        let access_token = self.access_tokens.resolve(&auth)?;
        let mut builder = self.client.get(url).bearer_auth(access_token.expose());
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let mut response = builder.send().map_err(|error| {
            if error.is_timeout() {
                ConnectorHttpFailure::Timeout
            } else {
                ConnectorHttpFailure::Network
            }
        })?;
        let status = response.status().as_u16();
        let mut headers = BTreeMap::new();
        if let Some(retry_after) = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
        {
            headers.insert("Retry-After".to_string(), retry_after.to_string());
        }
        if response
            .content_length()
            .is_some_and(|length| length > request.max_response_bytes as u64)
        {
            return Err(ConnectorHttpFailure::ResponseTooLarge);
        }
        let mut body = Vec::with_capacity(request.max_response_bytes.min(8192));
        let read_result = response
            .by_ref()
            .take(request.max_response_bytes as u64 + 1)
            .read_to_end(&mut body);
        if let Err(error) = read_result {
            let failure = if error.kind() == std::io::ErrorKind::TimedOut {
                ConnectorHttpFailure::Timeout
            } else {
                ConnectorHttpFailure::Network
            };
            body.zeroize();
            return Err(failure);
        }
        if body.len() > request.max_response_bytes {
            body.zeroize();
            return Err(ConnectorHttpFailure::ResponseTooLarge);
        }
        Ok(ConnectorHttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
pub struct ScriptedConnectorHttpTransport {
    responses: std::sync::Mutex<
        std::collections::VecDeque<Result<ConnectorHttpResponse, ConnectorHttpFailure>>,
    >,
    requests: std::sync::Mutex<Vec<ConnectorHttpRequest>>,
    auth_contexts: std::sync::Mutex<Vec<ConnectorHttpAuthContext>>,
}

#[cfg(test)]
impl ScriptedConnectorHttpTransport {
    pub fn new(responses: Vec<Result<ConnectorHttpResponse, ConnectorHttpFailure>>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses.into()),
            requests: std::sync::Mutex::new(Vec::new()),
            auth_contexts: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn take_requests(&self) -> Vec<ConnectorHttpRequest> {
        std::mem::take(&mut *self.requests.lock().expect("request lock"))
    }

    pub fn take_auth_contexts(&self) -> Vec<ConnectorHttpAuthContext> {
        std::mem::take(&mut *self.auth_contexts.lock().expect("auth context lock"))
    }
}

#[cfg(test)]
impl ConnectorHttpTransport for ScriptedConnectorHttpTransport {
    fn execute(
        &self,
        auth: ConnectorHttpAuthContext,
        request: ConnectorHttpRequest,
    ) -> Result<ConnectorHttpResponse, ConnectorHttpFailure> {
        self.auth_contexts
            .lock()
            .expect("auth context lock")
            .push(auth);
        self.requests.lock().expect("request lock").push(request);
        self.responses
            .lock()
            .expect("response lock")
            .pop_front()
            .unwrap_or(Err(ConnectorHttpFailure::BeforeSend))
    }
}

#[cfg(test)]
pub fn json_response(status: u16, body: Value) -> ConnectorHttpResponse {
    ConnectorHttpResponse {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&body).expect("JSON response serializes"),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread;

    use super::*;

    struct StaticAccessTokenResolver {
        token: String,
        calls: AtomicUsize,
    }

    impl StaticAccessTokenResolver {
        fn new(token: &str) -> Self {
            Self {
                token: token.to_string(),
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl ConnectorAccessTokenResolver for StaticAccessTokenResolver {
        fn resolve(
            &self,
            _auth: &ConnectorHttpAuthContext,
        ) -> Result<ConnectorSecret, ConnectorHttpFailure> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            ConnectorSecret::new(self.token.clone())
                .map_err(|_| ConnectorHttpFailure::CredentialUnavailable)
        }
    }

    struct UnavailableAccessTokenResolver;

    impl ConnectorAccessTokenResolver for UnavailableAccessTokenResolver {
        fn resolve(
            &self,
            _auth: &ConnectorHttpAuthContext,
        ) -> Result<ConnectorSecret, ConnectorHttpFailure> {
            Err(ConnectorHttpFailure::CredentialUnavailable)
        }
    }

    fn auth_context() -> ConnectorHttpAuthContext {
        ConnectorHttpAuthContext {
            account_id: Uuid::new_v4(),
            credential_handle: ConnectorCredentialHandle::new(),
        }
    }

    fn request(url: String, maximum: usize) -> ConnectorHttpRequest {
        ConnectorHttpRequest {
            method: ConnectorHttpMethod::Get,
            url,
            headers: BTreeMap::new(),
            body: None,
            max_response_bytes: maximum,
        }
    }

    fn serve_once(response: Vec<u8>) -> (u16, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener binds");
        let port = listener.local_addr().expect("listener address").port();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request connects");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout sets");
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 1024];
            while bytes.len() < 8192 {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        bytes.extend_from_slice(&buffer[..read]);
                        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        });
        (port, receiver, handle)
    }

    #[test]
    fn reqwest_transport_injects_only_resolved_access_token() {
        let response =
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec();
        let (port, received, server) = serve_once(response);
        let resolver = Arc::new(StaticAccessTokenResolver::new("access-token-marker"));
        let transport = ReqwestConnectorHttpTransport::new_test_http(Arc::clone(&resolver), port)
            .expect("transport builds");
        let response = transport
            .execute(
                auth_context(),
                request(format!("http://127.0.0.1:{port}/v1.0/me"), 32),
            )
            .expect("request succeeds");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
        let raw_request = received
            .recv_timeout(Duration::from_secs(2))
            .expect("request captured")
            .to_ascii_lowercase();
        assert!(raw_request.contains("authorization: bearer access-token-marker"));
        assert!(!raw_request.contains("refresh-token-marker"));
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
        server.join().expect("server exits");
    }

    #[test]
    fn reqwest_transport_rejects_unsafe_request_before_resolving_credential() {
        let resolver = Arc::new(StaticAccessTokenResolver::new("access-token-marker"));
        let transport = ReqwestConnectorHttpTransport::new_test_http(Arc::clone(&resolver), 9)
            .expect("transport builds");
        for url in [
            "https://graph.microsoft.com/v1.0/me",
            "http://evil.example/v1.0/me",
            "http://user@127.0.0.1:9/v1.0/me",
            "http://127.0.0.1:9/v1.0/me#fragment",
        ] {
            assert!(matches!(
                transport.execute(auth_context(), request(url.to_string(), 32)),
                Err(ConnectorHttpFailure::InvalidRequest)
            ));
        }
        let mut unsafe_request = request("http://127.0.0.1:9/v1.0/me".to_string(), 32);
        unsafe_request
            .headers
            .insert("Authorization".to_string(), "Bearer injected".to_string());
        assert!(matches!(
            transport.execute(auth_context(), unsafe_request),
            Err(ConnectorHttpFailure::InvalidRequest)
        ));
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);

        let production = ReqwestConnectorHttpTransport::new_microsoft(Arc::clone(&resolver))
            .expect("production transport builds");
        for url in [
            "http://graph.microsoft.com/v1.0/me",
            "https://evil.example/v1.0/me",
            "https://graph.microsoft.com:444/v1.0/me",
        ] {
            assert!(matches!(
                production.execute(auth_context(), request(url.to_string(), 32)),
                Err(ConnectorHttpFailure::InvalidRequest)
            ));
        }
        assert!(matches!(
            production.execute(
                auth_context(),
                request(
                    "https://graph.microsoft.com/v1.0/me".to_string(),
                    MAX_CONNECTOR_HTTP_RESPONSE_BYTES + 1,
                ),
            ),
            Err(ConnectorHttpFailure::InvalidRequest)
        ));
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reqwest_transport_does_not_follow_redirects() {
        let response = b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec();
        let (port, received, server) = serve_once(response);
        let resolver = StaticAccessTokenResolver::new("access-token-marker");
        let transport =
            ReqwestConnectorHttpTransport::new_test_http(resolver, port).expect("transport builds");
        let response = transport
            .execute(
                auth_context(),
                request(format!("http://127.0.0.1:{port}/redirect"), 32),
            )
            .expect("redirect response returns without following");
        assert_eq!(response.status, 302);
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("first request captured");
        server.join().expect("server exits");
    }

    #[test]
    fn reqwest_transport_stops_at_streaming_response_limit() {
        let mut response = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        response.extend(std::iter::repeat(b'x').take(64));
        let (port, received, server) = serve_once(response);
        let resolver = StaticAccessTokenResolver::new("access-token-marker");
        let transport =
            ReqwestConnectorHttpTransport::new_test_http(resolver, port).expect("transport builds");
        assert!(matches!(
            transport.execute(
                auth_context(),
                request(format!("http://127.0.0.1:{port}/large"), 8),
            ),
            Err(ConnectorHttpFailure::ResponseTooLarge)
        ));
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("request captured");
        server.join().expect("server exits");

        let response =
            b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\n".to_vec();
        let (port, received, server) = serve_once(response);
        let transport = ReqwestConnectorHttpTransport::new_test_http(
            StaticAccessTokenResolver::new("access-token-marker"),
            port,
        )
        .expect("transport builds");
        assert!(matches!(
            transport.execute(
                auth_context(),
                request(format!("http://127.0.0.1:{port}/large"), 8),
            ),
            Err(ConnectorHttpFailure::ResponseTooLarge)
        ));
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("request captured");
        server.join().expect("server exits");
    }

    #[test]
    fn reqwest_transport_does_not_connect_without_a_credential() {
        let transport =
            ReqwestConnectorHttpTransport::new_test_http(UnavailableAccessTokenResolver, 9)
                .expect("transport builds");
        assert!(matches!(
            transport.execute(
                auth_context(),
                request("http://127.0.0.1:9/v1.0/me".to_string(), 32),
            ),
            Err(ConnectorHttpFailure::CredentialUnavailable)
        ));
    }
}
