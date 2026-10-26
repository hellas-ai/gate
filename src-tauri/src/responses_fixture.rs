//! Replaces only external Responses HTTP I/O; SDK preparation, projection,
//! attestation, authorization, channels, verification and settlement remain real.
use hellas_executor::{FetchProvider, FetchProviderFuture, PreparedFetchRequest};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Notify,
};
struct Driver {
    endpoint: reqwest::Url,
    client: reqwest::Client,
}
impl FetchProvider for Driver {
    fn execution_environment(&self) -> hellas_rpc::ContentId {
        hellas_reseller::responses_contract().unwrap().manifest_id()
    }
    fn run(&self, request: PreparedFetchRequest) -> FetchProviderFuture<'_> {
        Box::pin(async move {
            hellas_providers::execute_responses_request(
                &self.client,
                self.endpoint.clone(),
                "fixture-upstream",
                request.body.as_bytes().to_vec(),
                &request.idempotency_key(),
                "fixture Responses",
            )
            .await
        })
    }
}
pub fn entry(origin: &str) -> hellas_sdk::FetchRouteEntry {
    hellas_sdk::FetchRouteEntry::new(
        Arc::new(Driver {
            endpoint: format!("{origin}/v1/responses").parse().unwrap(),
            client: hellas_providers::responses_http_client(Default::default()).unwrap(),
        }),
        Arc::new(hellas_reseller::OpenAiResponsesAdaptorFactory::new().unwrap()),
        hellas_sdk::FetchRoutePolicy::default(),
    )
    .unwrap()
}
pub struct Origin {
    pub url: String,
    pub hold: Arc<AtomicBool>,
    pub started: Arc<Notify>,
    pub release: Arc<Notify>,
    pub calls: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Origin {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hold = Arc::new(AtomicBool::new(false));
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (h, s, r, c) = (
            hold.clone(),
            started.clone(),
            release.clone(),
            calls.clone(),
        );
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let (h, s, r, c) = (h.clone(), s.clone(), r.clone(), c.clone());
                tokio::spawn(async move {
                    let mut head = vec![];
                    while !head.ends_with(b"\r\n\r\n") {
                        head.push(stream.read_u8().await.unwrap());
                        assert!(head.len() <= 8192);
                    }
                    let head = String::from_utf8(head).unwrap();
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    assert!(length <= 64 << 10);
                    let mut body = vec![0; length];
                    stream.read_exact(&mut body).await.unwrap();
                    c.fetch_add(1, Ordering::SeqCst);
                    s.notify_one();
                    if h.load(Ordering::SeqCst) {
                        r.notified().await;
                    }
                    let body = response_events();
                    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
                });
            }
        });
        Self {
            url,
            hold,
            started,
            release,
            calls,
            task,
        }
    }
}
impl Drop for Origin {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn response_events() -> String {
    use serde_json::json;
    let part = json!({"type":"output_text","text":"hello","annotations":[]});
    let message = json!({"type":"message","id":"msg_fixture","status":"completed","role":"assistant","content":[part.clone()]});
    [
        json!({"type":"response.created","response":{"id":"resp_fixture","object":"response","status":"in_progress","model":"fixture","output":[]}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_fixture","status":"in_progress","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","item_id":"msg_fixture","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        json!({"type":"response.output_text.delta","item_id":"msg_fixture","output_index":0,"content_index":0,"delta":"hello"}),
        json!({"type":"response.output_text.done","item_id":"msg_fixture","output_index":0,"content_index":0,"text":"hello"}),
        json!({"type":"response.content_part.done","item_id":"msg_fixture","output_index":0,"content_index":0,"part":part}),
        json!({"type":"response.output_item.done","output_index":0,"item":message.clone()}),
        json!({"type":"response.completed","response":{"id":"resp_fixture","object":"response","status":"completed","model":"fixture","output":[message],"usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}}),
    ].into_iter().enumerate().map(|(index,mut event)|{event["sequence_number"]=index.into();format!("event: {}\ndata: {event}\n\n",event["type"].as_str().unwrap())}).collect()
}
