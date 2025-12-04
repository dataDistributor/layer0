use dxid_node::run_node;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let path = std::env::var("DXID_CONFIG").unwrap_or_else(|_| "config/dxid.toml".to_string());
    if let Err(e) = run_node(PathBuf::from(path)).await {
        eprintln!("node failed: {e:?}");
    }
}
