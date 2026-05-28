//! Islands architecture

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HydrationMode {
    #[default]
    Eager,
    Lazy,
    Interaction,
    Visible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Island;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandProps;
#[derive(Debug, Clone)]
pub struct IslandRenderer;
#[derive(Debug, Clone)]
pub struct IslandRegistry {
    manifests: HashMap<String, IslandManifest>,
}

impl IslandRegistry {
    pub fn new() -> Self {
        Self {
            manifests: HashMap::new(),
        }
    }
    pub fn register(&mut self, _name: &str, _config: IslandConfig) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandManifest {
    pub islands: HashMap<String, IslandConfig>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandConfig {
    pub name: String,
    pub hydration: HydrationMode,
    pub props: HashMap<String, serde_json::Value>,
}

impl IslandManifest {
    pub fn new() -> Self {
        Self {
            islands: HashMap::new(),
        }
    }
    pub fn register(&mut self, name: String, config: IslandConfig) {
        self.islands.insert(name, config);
    }
    pub fn get(&self, name: &str) -> Option<&IslandConfig> {
        self.islands.get(name)
    }
}
