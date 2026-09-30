//! One executing chain node shared by Gate's paid roles, started lazily.
use std::path::PathBuf;
use tokio::sync::OnceCell;

pub struct ChainNode {
    directory: PathBuf,
    node: OnceCell<hellas_sdk::FullNode>,
}
impl ChainNode {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            node: OnceCell::new(),
        }
    }
    pub async fn get(
        &self,
        config: &hellas_sdk::work_config::WorkConfig,
    ) -> anyhow::Result<hellas_sdk::FullNode> {
        let node = self
            .node
            .get_or_try_init(|| async {
                Ok::<_, anyhow::Error>(
                    hellas_sdk::FullNode::start(config.node_config(self.directory.clone(), None)?)
                        .await?,
                )
            })
            .await?;
        config.check_node(node)?;
        Ok(node.clone())
    }
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        if let Some(node) = self.node.get() {
            node.clone().shutdown().await?;
        }
        Ok(())
    }
}
