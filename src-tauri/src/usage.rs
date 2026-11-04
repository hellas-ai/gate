//! Local key-use metadata; all request execution and authority stay in the SDK.
use crate::provider_setup::Composition;
use hellas_executor::{FetchProvider, FetchProviderFuture, PreparedFetchRequest};
use std::{collections::BTreeMap, sync::Arc};
pub enum Key {
    Fixed(String, String),
    Http(BTreeMap<String, (String, String)>),
}
struct Usage {
    inner: Arc<dyn FetchProvider>,
    composition: Composition,
    key: Key,
}
pub fn track(
    inner: Arc<dyn FetchProvider>,
    composition: Composition,
    key: Key,
) -> Arc<dyn FetchProvider> {
    Arc::new(Usage {
        inner,
        composition,
        key,
    })
}
impl FetchProvider for Usage {
    fn execution_environment(&self) -> hellas_rpc::ContentId {
        self.inner.execution_environment()
    }
    fn run(&self, request: PreparedFetchRequest) -> FetchProviderFuture<'_> {
        Box::pin(async move {
            let reference = match &self.key {
                Key::Fixed(reference, fingerprint) => {
                    Some((reference.clone(), fingerprint.clone()))
                }
                Key::Http(keys) => {
                    hellas_rpc::http_fetch::HttpFetchRequest::decode(request.body.as_bytes())
                        .ok()
                        .and_then(|request| {
                            request
                                .credential
                                .and_then(|alias| keys.get(&alias).cloned())
                        })
                }
            };
            let result = self.inner.run(request).await;
            if result.is_ok()
                && let Some((reference, fingerprint)) = reference
            {
                let composition = self.composition.clone();
                match tokio::task::spawn_blocking(move || {
                    composition.used(&reference, &fingerprint)
                })
                .await
                {
                    Ok(Ok(())) => {}
                    _ => tracing::warn!(
                        "upstream request succeeded but key-use metadata could not be saved"
                    ),
                }
            }
            result
        })
    }
}
