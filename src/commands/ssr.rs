//! Server-Side Rendering

use crate::transpile::TsParser;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize)]
pub struct IslandManifestEntry {
    pub name: String,
    pub hash: String,
    pub props: Vec<String>,
    pub module_path: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct IslandManifest {
    #[serde(default)]
    pub islands: HashMap<String, IslandManifestEntry>,
}

impl IslandManifest {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_island(&mut self, name: String, props: Vec<String>) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        let hash = format!("{:x}", hasher.finish())[..8].to_string();
        let entry = IslandManifestEntry {
            name: name.clone(),
            hash: hash.clone(),
            props,
            module_path: format!("/_runts/islands/{}.{}.js", name, hash),
        };
        self.islands.insert(name, entry);
        hash
    }
}

pub fn method_to_str(m: &super::routes::HttpMethod) -> &'static str {
    match m {
        super::routes::HttpMethod::GET => "GET",
        super::routes::HttpMethod::POST => "POST",
        super::routes::HttpMethod::PUT => "PUT",
        super::routes::HttpMethod::DELETE => "DELETE",
        super::routes::HttpMethod::PATCH => "PATCH",
        super::routes::HttpMethod::HEAD => "HEAD",
        super::routes::HttpMethod::OPTIONS => "OPTIONS",
    }
}

pub async fn render_page(route: &str, _props: serde_json::Value) -> anyhow::Result<String> {
    Ok(format!(
        "<html><body><div id=\"app\">Hello from {}</div></body></html>",
        route
    ))
}
pub async fn render_island(name: &str, props: &serde_json::Value) -> anyhow::Result<String> {
    Ok(format!(
        "<div data-island=\"{}\">Island: {} props: {:?}</div>",
        name, name, props
    ))
}
