//! **The older HTTP+SSE transport** (MCP 2024-11-05, deprecated, which rmcp
//! no longer has): a `GET` opens a stream of events; its first, `endpoint`,
//! names where to `POST` each message; every answer comes back on the
//! stream as a `message` event. For a link that is such a stream, or one
//! whose Streamable HTTP `POST` answered 404 or 405 (`remote.rs`).
//!
//! The same [`Http`] as Streamable HTTP (its receipts, its rules, its
//! secrets), and one rule more: the `endpoint` must be on the stream's own
//! origin, or Yap stops (a token never goes to another server).

use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::service::RoleClient;
use rmcp::transport::streamable_http_client::StreamableHttpError;
use url::Url;

use super::{Http, HttpError};

/// A conversation over the older transport.
pub struct Legacy {
    http: Http,
    endpoint: Arc<str>,
    rx: tokio::sync::mpsc::Receiver<ServerJsonRpcMessage>,
    reader: tokio::task::JoinHandle<()>,
}

impl Drop for Legacy {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl Legacy {
    /// Open `url`'s stream and wait (at most `wait`) for its endpoint.
    pub async fn connect(http: Http, url: &Url, wait: Duration) -> Result<Legacy, HttpError> {
        let host = http.host().to_string();
        let mut events = http
            .open_stream(url.as_str(), None, None, None, Default::default(), super::MAX_BODY)
            .await
            .map_err(|e| match e {
                rmcp::transport::streamable_http_client::StreamableHttpError::Client(h) => h,
                rmcp::transport::streamable_http_client::StreamableHttpError::ServerDoesNotSupportSse => HttpError::NotStreamable { host: host.clone(), status: 405 },
                rmcp::transport::streamable_http_client::StreamableHttpError::AuthRequired(_) => HttpError::TokenRefused { host: host.clone(), status: 401 },
                other => HttpError::Other(format!("{host} didn't open an MCP stream ({other}).")),
            })?;
        let endpoint = tokio::time::timeout(wait, async {
            while let Some(event) = events.next().await {
                let event = event.map_err(|_| HttpError::Unreachable { host: host.clone() })?;
                if event.event.as_deref() == Some("endpoint") {
                    return Ok(event.data.unwrap_or_default());
                }
            }
            Err(HttpError::Other(format!("{host} closed the stream before saying where to send messages.")))
        })
        .await
        .map_err(|_| HttpError::TimedOut { host: host.clone() })??;
        let endpoint = url.join(endpoint.trim()).map_err(|_| HttpError::Other(format!("{host} named an address Yap couldn't read.")))?;
        if endpoint.origin() != url.origin() {
            return Err(HttpError::OffHost { host: host.clone(), to: endpoint.host_str().unwrap_or("another address").to_string() });
        }
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let reader = tokio::spawn(async move {
            while let Some(Ok(event)) = events.next().await {
                if !matches!(event.event.as_deref(), None | Some("message")) {
                    continue;
                }
                let Some(data) = event.data else { continue };
                match serde_json::from_str::<ServerJsonRpcMessage>(&data) {
                    Ok(m) => {
                        if tx.send(m).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => tracing::debug!("connectors: an older-transport event wasn't a message"),
                }
            }
        });
        Ok(Legacy { http, endpoint: Arc::from(endpoint.as_str()), rx, reader })
    }
}

impl Http {
    /// One message `POST`ed to the older transport's endpoint (its answer
    /// comes on the stream).
    async fn post_legacy(&self, endpoint: &str, message: &ClientJsonRpcMessage) -> Result<(), HttpError> {
        let body = serde_json::to_vec(message).map_err(|_| HttpError::Other("Yap couldn't write the message.".into()))?;
        let token = self.bearer(None).await.map_err(|e| match e {
            StreamableHttpError::Client(h) => h,
            _ => HttpError::SignInEnded { host: self.host().to_string() },
        })?;
        let req = self.inner.client.post(endpoint).header(ACCEPT, super::JSON).header(CONTENT_TYPE, super::JSON).body(body);
        let req = self.credentials(req, token.as_deref());
        let resp = self.send(req).await?;
        let status = resp.status().as_u16();
        if resp.status().is_success() {
            return Ok(());
        }
        if status == 401 || status == 403 {
            return Err(HttpError::TokenRefused { host: self.host().to_string(), status });
        }
        Err(HttpError::Status { host: self.host().to_string(), status })
    }
}

impl rmcp::transport::Transport<RoleClient> for Legacy {
    type Error = HttpError;

    fn send(&mut self, item: ClientJsonRpcMessage) -> impl std::future::Future<Output = Result<(), HttpError>> + Send + 'static {
        let http = self.http.clone();
        let endpoint = self.endpoint.clone();
        async move { http.post_legacy(&endpoint, &item).await }
    }

    fn receive(&mut self) -> impl std::future::Future<Output = Option<ServerJsonRpcMessage>> + Send {
        self.rx.recv()
    }

    fn close(&mut self) -> impl std::future::Future<Output = Result<(), HttpError>> + Send {
        self.reader.abort();
        std::future::ready(Ok(()))
    }
}
