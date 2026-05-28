//! HTTP request handlers

use crate::config::Config;
use anyhow::Result;
use std::path::PathBuf;

fn extract_headers(_headers: &http::HeaderMap) -> std::collections::HashMap<String, String> {
    std::collections::HashMap::new()
}
fn path_to_title(path: &str) -> String {
    if path == "/" {
        "Home".to_string()
    } else {
        path.trim_start_matches('/')
            .replace('/', " - ")
            .split('-')
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().to_string() + &c.as_str().to_lowercase())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}
fn build_nav_links() -> String {
    r#"<nav class="runts-nav"><a href="/">Home</a><a href="/blog">Blog</a></nav>"#.to_string()
}

pub async fn run_server(_config: &Config, port: u16) -> Result<()> {
    let project_root = PathBuf::from(".");
    tracing::info!("Starting dev server on port {}", port);
    tokio::signal::ctrl_c().await?;
    Ok(())
}
