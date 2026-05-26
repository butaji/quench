//! Islands architecture implementation
//!
//! Islands are interactive components that are hydrated on the client.
//! Static content is rendered once on the server and never shipped to the client.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// Island hydration mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IslandMode {
    /// Hydrate immediately on page load
    /// Use for: forms, critical UI elements
    Eager,
    
    /// Hydrate when the island enters the viewport
    /// Use for: below-the-fold content
    Lazy,
    
    /// Hydrate on first user interaction (click, focus, hover)
    /// Use for: modals, tooltips, dropdowns
    Interaction,
    
    /// Hydrate when the island becomes visible in the DOM
    /// Use for: dynamically added content
    Visible,
}

impl Default for IslandMode {
    fn default() -> Self {
        IslandMode::Lazy
    }
}

/// Island configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandConfig {
    /// Island name (component name)
    pub name: String,
    
    /// Unique ID for this island instance
    pub id: String,
    
    /// Props serialized as JSON
    pub props: serde_json::Value,
    
    /// Hydration mode
    pub mode: IslandMode,
    
    /// Whether this island uses signals
    pub has_signals: bool,
}

impl IslandConfig {
    /// Create a new island configuration
    pub fn new(name: impl Into<String>, props: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            id: generate_island_id(),
            props,
            mode: IslandMode::default(),
            has_signals: false,
        }
    }
    
    /// Create an eager island
    pub fn eager(name: impl Into<String>, props: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            id: generate_island_id(),
            props,
            mode: IslandMode::Eager,
            has_signals: false,
        }
    }
    
    /// Set the hydration mode
    pub fn mode(mut self, mode: IslandMode) -> Self {
        self.mode = mode;
        self
    }
    
    /// Mark this island as using signals
    pub fn with_signals(mut self) -> Self {
        self.has_signals = true;
        self
    }
}

/// Generate a unique island ID
fn generate_island_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    
    format!("island-{:x}-{:x}", timestamp, id)
}

/// Island container HTML for SSR
#[derive(Debug, Clone)]
pub struct IslandContainer {
    /// Configuration
    pub config: IslandConfig,
    
    /// Server-rendered HTML placeholder
    pub placeholder: String,
}

impl IslandContainer {
    /// Create a new island container
    pub fn new(config: IslandConfig, placeholder: String) -> Self {
        Self { config, placeholder }
    }
    
    /// Render the island container HTML
    ///
    /// This generates the HTML that will be replaced by client-side hydration.
    /// The output includes:
    /// - Wrapper div with data attributes
    /// - Serialized props in a script tag
    /// - Server-rendered placeholder
    pub fn render_to_html(&self) -> String {
        let mode_attr = match self.config.mode {
            IslandMode::Eager => "eager",
            IslandMode::Lazy => "lazy",
            IslandMode::Interaction => "interaction",
            IslandMode::Visible => "visible",
        };
        
        let props_json = serde_json::to_string(&self.config.props)
            .unwrap_or_else(|_| "{}".to_string());
        
        format!(
            r#"<div data-island="{}" data-id="{}" data-mode="{}" data-has-signals="{}">
    <script type="application/x-runts-island" data-props="true">{}</script>
    {}
</div>"#,
            self.config.name,
            self.config.id,
            mode_attr,
            self.config.has_signals,
            props_json,
            self.placeholder
        )
    }
}

/// Island registry for tracking all islands
#[derive(Debug, Default)]
pub struct IslandRegistry {
    /// All registered islands
    islands: HashMap<String, IslandEntry>,
}

#[derive(Debug, Clone)]
struct IslandEntry {
    /// Component name
    name: String,
    
    /// File path
    path: String,
    
    /// Whether it's a client-side island
    is_client: bool,
}

impl IslandRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Register an island component
    pub fn register(&mut self, name: impl Into<String>, path: impl Into<String>) {
        let name = name.into();
        let path = path.into();
        
        self.islands.insert(
            name.clone(),
            IslandEntry {
                name,
                path,
                is_client: true,
            },
        );
    }
    
    /// Get all registered island names
    pub fn island_names(&self) -> Vec<&str> {
        self.islands.keys().map(|s| s.as_str()).collect()
    }
    
    /// Check if an island is registered
    pub fn is_registered(&self, name: &str) -> bool {
        self.islands.contains_key(name)
    }
    
    /// Get island info
    pub fn get(&self, name: &str) -> Option<&IslandEntry> {
        self.islands.get(name)
    }
}

/// Server-side rendering helpers for islands
pub mod ssr {
    
    use serde_json::Value;
    
    /// Render props as JSON for client hydration
    pub fn serialize_props(props: &Value) -> String {
        serde_json::to_string(props).unwrap_or_else(|_| "{}".to_string())
    }
    
    /// Check if a value can be serialized for island props
    pub fn is_serializable(value: &Value) -> bool {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
            Value::Array(arr) => arr.iter().all(is_serializable),
            Value::Object(obj) => obj.values().all(is_serializable),
        }
    }
    
    /// Render a static placeholder for an island
    ///
    /// This is used when the island cannot be pre-rendered on the server.
    /// The placeholder shows a loading state that will be replaced by hydration.
    pub fn render_placeholder(name: &str) -> String {
        format!(
            r#"<div class="runts-island-placeholder" data-island="{}">
    <noscript>
        <p>JavaScript is required for this interactive component.</p>
    </noscript>
</div>"#,
            name
        )
    }
}

/// Client-side hydration types (for documentation)
/// 
/// These types are used in the client-side runtime (JavaScript)
/// and are documented here for completeness.
pub mod client {
    /// Hydration options for an island
    #[derive(Debug, Clone)]
    pub struct HydrationOptions {
        /// Container element (string ID for server-side)
        pub container_id: String,
        
        /// Props from server
        pub props: serde_json::Value,
        
        /// Whether to immediately hydrate
        pub immediate: bool,
    }
    
    /// Client-side island instance
    /// 
    /// This trait is implemented by JavaScript island bundles
    /// and documented here for type consistency.
    pub trait IslandInstance {
        /// Mount the island
        /// 
        /// In the JS runtime, this replaces the server-rendered
        /// placeholder with the interactive island.
        fn mount(&self, container_id: &str, props: serde_json::Value);
        
        /// Unmount the island
        fn unmount(&self);
        
        /// Update props
        fn update(&self, props: serde_json::Value);
    }
}

/// Bundle information for islands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandBundle {
    /// Island name
    pub name: String,
    
    /// JavaScript bundle path
    pub js_path: String,
    
    /// Bundle size in bytes
    pub size: usize,
    
    /// Dependencies
    pub deps: Vec<String>,
}

impl IslandBundle {
    /// Create bundle info
    pub fn new(name: impl Into<String>, js_path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            js_path: js_path.into(),
            size: 0,
            deps: Vec::new(),
        }
    }
    
    /// Set bundle size
    pub fn with_size(mut self, size: usize) -> Self {
        self.size = size;
        self
    }
    
    /// Add a dependency
    pub fn with_dep(mut self, dep: impl Into<String>) -> Self {
        self.deps.push(dep.into());
        self
    }
}

/// Manifest of all islands for the client runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandsManifest {
    /// All island bundles
    pub bundles: Vec<IslandBundle>,
    
    /// Runtime version
    pub version: String,
}

impl IslandsManifest {
    /// Create a new manifest
    pub fn new() -> Self {
        Self {
            bundles: Vec::new(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
    
    /// Add a bundle
    pub fn add_bundle(&mut self, bundle: IslandBundle) {
        self.bundles.push(bundle);
    }
    
    /// Serialize to JSON for client
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

impl Default for IslandsManifest {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    
    #[test]
    fn test_island_config() {
        let config = IslandConfig::new("Counter", json!({"initial": 42}));
        assert_eq!(config.name, "Counter");
        assert!(config.id.starts_with("island-"));
    }
    
    #[test]
    fn test_island_container() {
        let config = IslandConfig::eager("Counter", json!({"initial": 42}));
        let container = IslandContainer::new(config, "<span>42</span>".to_string());
        
        let html = container.render_to_html();
        assert!(html.contains("data-island=\"Counter\""));
        assert!(html.contains("data-mode=\"eager\""));
        assert!(html.contains("application/x-runts-island"));
    }
    
    #[test]
    fn test_serializable() {
        // Null IS serializable in JSON
        assert!(ssr::is_serializable(&json!(null)));
        assert!(ssr::is_serializable(&json!(42)));
        assert!(ssr::is_serializable(&json!("hello")));
        assert!(ssr::is_serializable(&json!({"key": "value"})));
        // Complex nested values
        assert!(ssr::is_serializable(&json!({"nested": {"value": [1, 2, 3]}})));
    }
}
