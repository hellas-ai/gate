use futures::StreamExt;
use gate_core::router::transport::{BodyStream, Transport, TransportRequest, TransportResponse};
use gate_core::{Error, Result};
use http::StatusCode;
use reqwest::Client;

/// HTTP transport using reqwest
pub struct HttpTransport {
    client: Client,
}

impl HttpTransport {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl Transport for HttpTransport {
    async fn post_json(&self, req: TransportRequest) -> Result<TransportResponse> {
        let mut builder = self.client.post(&req.url).json(&req.body);
        // Apply headers
        for (name, value) in req.headers.iter() {
            builder = builder.header(name, value);
        }

        if let Some(timeout) = req.timeout {
            builder = builder.timeout(timeout);
        }

        let resp = builder
            .send()
            .await
            .map_err(|e| Error::ServiceUnavailable(format!("Failed to send request: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(Error::Rejected(code, body));
        }

        // Convert headers
        let mut headers = std::collections::HashMap::new();
        for (k, v) in resp.headers().iter() {
            if let Ok(s) = v.to_str() {
                headers.insert(k.to_string(), s.to_string());
            }
        }

        let stream = resp.bytes_stream().map(|r| match r {
            Ok(b) => Ok(b.to_vec()),
            Err(e) => Err(Error::ServiceUnavailable(format!("Body error: {e}"))),
        });

        let body: BodyStream = Box::pin(stream);
        Ok(TransportResponse {
            status: status.as_u16(),
            headers,
            body,
        })
    }
}
