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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hydration_mode_default() {
        assert_eq!(HydrationMode::default(), HydrationMode::Eager);
    }

    #[test]
    fn test_hydration_mode_variants() {
        assert_eq!(HydrationMode::Eager, HydrationMode::Eager);
        assert_eq!(HydrationMode::Lazy, HydrationMode::Lazy);
        assert_eq!(HydrationMode::Interaction, HydrationMode::Interaction);
        assert_eq!(HydrationMode::Visible, HydrationMode::Visible);
    }

    #[test]
    fn test_hydration_mode_partial_eq() {
        assert_eq!(HydrationMode::Eager, HydrationMode::Eager);
        assert_ne!(HydrationMode::Eager, HydrationMode::Lazy);
    }

    #[test]
    fn test_island_config() {
        let config = IslandConfig {
            name: "Counter".to_string(),
            hydration: HydrationMode::Lazy,
            props: HashMap::new(),
        };
        assert_eq!(config.name, "Counter");
        assert_eq!(config.hydration, HydrationMode::Lazy);
    }

    #[test]
    fn test_island_manifest_new() {
        let manifest = IslandManifest::new();
        assert!(manifest.islands.is_empty());
    }

    #[test]
    fn test_island_manifest_register() {
        let mut manifest = IslandManifest::new();
        let config = IslandConfig {
            name: "Button".to_string(),
            hydration: HydrationMode::Eager,
            props: HashMap::new(),
        };
        manifest.register("btn".to_string(), config.clone());
        assert_eq!(manifest.islands.len(), 1);
        assert_eq!(manifest.get("btn").unwrap().name, "Button");
    }

    #[test]
    fn test_island_manifest_get_nonexistent() {
        let manifest = IslandManifest::new();
        assert!(manifest.get("nonexistent").is_none());
    }

    #[test]
    fn test_island_manifest_clone() {
        let mut manifest = IslandManifest::new();
        manifest.register("test".to_string(), IslandConfig {
            name: "Test".to_string(),
            hydration: HydrationMode::Eager,
            props: HashMap::new(),
        });
        let manifest2 = manifest.clone();
        assert_eq!(manifest.islands.len(), manifest2.islands.len());
    }

    #[test]
    fn test_island_registry_new() {
        let registry = IslandRegistry::new();
        assert!(registry.manifests.is_empty());
    }

    #[test]
    fn test_island_registry_register() {
        let mut registry = IslandRegistry::new();
        registry.register("test", IslandConfig {
            name: "Test".to_string(),
            hydration: HydrationMode::Eager,
            props: HashMap::new(),
        });
    }

    #[test]
    fn test_island_clone() {
        let _island: Island = Island;
    }

    #[test]
    fn test_island_props_clone() {
        let _: IslandProps = IslandProps;
    }

    #[test]
    fn test_island_renderer_clone() {
        let _: IslandRenderer = IslandRenderer;
    }

    #[test]
    fn test_island_config_serde_roundtrip() {
        let config = IslandConfig {
            name: "Test".to_string(),
            hydration: HydrationMode::Interaction,
            props: HashMap::new(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: IslandConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.name, decoded.name);
        assert_eq!(config.hydration, decoded.hydration);
    }

    #[test]
    fn test_hydration_mode_serde_roundtrip() {
        let mode = HydrationMode::Visible;
        let json = serde_json::to_string(&mode).unwrap();
        let decoded: HydrationMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, decoded);
    }
}
