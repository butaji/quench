//! Islands architecture implementation
//!
//! Islands provide selective hydration for interactive components.
//! Each island is a Preact component that hydrates on the client.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// Hydration strategy for islands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HydrationStrategy {
    /// Hydrate immediately on page load
    Eager,
    /// Hydrate when the island becomes visible
    Visible,
    /// Hydrate during browser idle time
    Idle,
    /// Hydrate only on user interaction
    Manual,
    /// Never hydrate (static only)
    Static,
}

impl Default for HydrationStrategy {
    fn default() -> Self {
        Self::Visible
    }
}

/// Island descriptor for the registry
#[derive(Debug, Clone)]
pub struct IslandDescriptor {
    /// Unique island name
    pub name: String,
    /// Props schema for serialization
    pub props_type: String,
    /// Hydration strategy
    pub strategy: HydrationStrategy,
    /// Import path for client bundle
    pub import_path: String,
    /// Server-renderable (true for most islands)
    pub ssr_capable: bool,
}

/// Island instance during SSR
#[derive(Debug, Clone)]
pub struct IslandInstance {
    /// The island name
    pub name: String,
    /// Serialized props (JSON)
    pub props: String,
    /// Hydration strategy
    pub strategy: HydrationStrategy,
    /// SSR-rendered HTML content
    pub html: String,
    /// Container element selector
    pub selector: String,
}

/// Island registry for managing all islands
pub struct IslandRegistry {
    /// Registered islands
    islands: HashMap<String, IslandDescriptor>,
    /// Pending instances for current page
    instances: Arc<RwLock<Vec<IslandInstance>>>,
    /// Island counter for unique IDs
    counter: Arc<RwLock<usize>>,
}

impl Default for IslandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl IslandRegistry {
    /// Create a new island registry
    pub fn new() -> Self {
        Self {
            islands: HashMap::new(),
            instances: Arc::new(RwLock::new(Vec::new())),
            counter: Arc::new(RwLock::new(0)),
        }
    }

    /// Register an island component
    pub fn register(&mut self, descriptor: IslandDescriptor) {
        self.islands.insert(descriptor.name.clone(), descriptor);
    }

    /// Register a default island
    pub fn register_default(&mut self, name: &str) {
        self.register(IslandDescriptor {
            name: name.to_string(),
            props_type: "serde_json::Value".to_string(),
            strategy: HydrationStrategy::Visible,
            import_path: format!("/islands/{}.js", name.to_lowercase()),
            ssr_capable: true,
        });
    }

    /// Create an island instance for SSR
    pub fn create_instance(
        &self,
        name: &str,
        props: impl Serialize,
        html: String,
    ) -> Option<IslandInstance> {
        let descriptor = self.islands.get(name)?;

        let props_json = serde_json::to_string(&props).ok()?;
        let id = {
            let mut counter = self.counter.write();
            *counter += 1;
            *counter
        };

        let instance = IslandInstance {
            name: name.to_string(),
            props: props_json,
            strategy: descriptor.strategy,
            html,
            selector: format!("[data-island=\"{}\"][data-id=\"{}\"]", name, id),
        };

        // Store for later hydration
        let mut instances = self.instances.write();
        instances.push(instance.clone());

        Some(instance)
    }

    /// Render island placeholder for SSR
    pub fn render_placeholder(&self, instance: &IslandInstance) -> String {
        let attrs = format!(
            r#"data-island="{}" data-id="{}" data-props="{}" data-strategy="{}""#,
            instance.name,
            instance.selector.split('=').last().unwrap_or("0"),
            html_escape(&instance.props),
            format!("{:?}", instance.strategy).to_lowercase(),
        );

        format!(
            r#"<div class="__island__" {}><div class="__island-content__">{}</div></div>"#,
            attrs,
            instance.html
        )
    }

    /// Get all instances for current page
    pub fn get_instances(&self) -> Vec<IslandInstance> {
        self.instances.read().clone()
    }

    /// Clear instances for next page
    pub fn clear(&self) {
        self.instances.write().clear();
    }

    /// Generate hydration manifest for the page
    pub fn generate_manifest(&self) -> IslandManifest {
        let instances = self.get_instances();
        let islands: Vec<IslandManifestEntry> = instances
            .iter()
            .map(|i| IslandManifestEntry {
                name: i.name.clone(),
                selector: i.selector.clone(),
                props: i.props.clone(),
                strategy: i.strategy,
            })
            .collect();

        IslandManifest { islands }
    }

    /// Check if an island is registered
    pub fn is_registered(&self, name: &str) -> bool {
        self.islands.contains_key(name)
    }
}

/// Island manifest for client hydration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandManifest {
    pub islands: Vec<IslandManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandManifestEntry {
    pub name: String,
    pub selector: String,
    pub props: String,
    pub strategy: HydrationStrategy,
}

/// Client-side island hydrator
pub struct IslandHydrator {
    /// Registered island loaders (name -> loader)
    #[allow(dead_code)]
    loaders: HashMap<String, String>, // Simplified: stores JS code for now
}

impl Default for IslandHydrator {
    fn default() -> Self {
        Self::new()
    }
}

impl IslandHydrator {
    pub fn new() -> Self {
        Self {
            loaders: HashMap::new(),
        }
    }

    /// Register an island loader
    #[allow(dead_code)]
    pub fn register(&mut self, name: &str, js_module: &str) {
        self.loaders.insert(name.to_string(), js_module.to_string());
    }

    /// Hydrate a single island (client-side only)
    #[allow(dead_code)]
    pub fn hydrate(&self, name: &str, _selector: &str, _props_json: &str) -> Result<(), String> {
        if !self.loaders.contains_key(name) {
            // For now, log that hydration would happen client-side
            tracing::debug!("Island '{}' would be hydrated client-side", name);
        }
        Ok(())
    }

    /// Hydrate all visible islands
    pub fn hydrate_visible(&self, manifest: &IslandManifest) {
        for entry in &manifest.islands {
            if entry.strategy == HydrationStrategy::Visible {
                if let Err(e) = self.hydrate(&entry.name, &entry.selector, &entry.props) {
                    eprintln!("[runts] Hydration error for {}: {}", entry.name, e);
                }
            }
        }
    }

    /// Schedule idle hydration
    pub fn schedule_idle(&self, manifest: &IslandManifest) {
        // In real implementation, this would use requestIdleCallback
        for entry in &manifest.islands {
            if entry.strategy == HydrationStrategy::Idle {
                if let Err(e) = self.hydrate(&entry.name, &entry.selector, &entry.props) {
                    eprintln!("[runts] Hydration error for {}: {}", entry.name, e);
                }
            }
        }
    }
}

/// Trait for client-side island instances
pub trait IslandInstanceTrait: Send + Sync {
    /// Mount the island to a DOM element
    fn mount(&self, selector: &str) -> Result<(), String>;

    /// Unmount and cleanup
    fn unmount(&self);

    /// Get the island name
    fn name(&self) -> &str;
}

/// Island hydration manager for SSR pages
pub struct IslandHydrationManager {
    /// Strategy observers
    observers: Vec<Box<dyn IslandObserver>>,
}

impl Default for IslandHydrationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl IslandHydrationManager {
    pub fn new() -> Self {
        Self {
            observers: Vec::new(),
        }
    }

    /// Add an observer for hydration events
    pub fn add_observer(&mut self, observer: Box<dyn IslandObserver>) {
        self.observers.push(observer);
    }

    /// Notify observers of hydration start
    pub fn notify_hydrating(&self, name: &str) {
        for obs in &self.observers {
            obs.on_hydrating(name);
        }
    }

    /// Notify observers of hydration complete
    pub fn notify_hydrated(&self, name: &str) {
        for obs in &self.observers {
            obs.on_hydrated(name);
        }
    }

    /// Notify observers of hydration error
    pub fn notify_error(&self, name: &str, error: &str) {
        for obs in &self.observers {
            obs.on_error(name, error);
        }
    }
}

/// Observer for island hydration events
pub trait IslandObserver: Send + Sync {
    fn on_hydrating(&self, _name: &str) {}
    fn on_hydrated(&self, _name: &str) {}
    fn on_error(&self, _name: &str, _error: &str) {}
}

/// HTML escape utility
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Preact signal integration for islands
#[cfg(feature = "signals")]
pub mod signal_integration {
    use super::*;
    use crate::runtime::signals::Signal;

    /// Signal-based island state
    pub struct SignalIslandState<T: Clone> {
        pub value: Signal<T>,
        pub hydrated: Signal<bool>,
    }

    impl<T: Clone + 'static> SignalIslandState<T> {
        pub fn new(initial: T) -> Self {
            Self {
                value: Signal::new(initial),
                hydrated: Signal::new(false),
            }
        }

        pub fn mark_hydrated(&self) {
            self.hydrated.set(true);
        }

        pub fn is_hydrated(&self) -> bool {
            self.hydrated.get()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_island_registration() {
        let mut registry = IslandRegistry::new();
        registry.register(IslandDescriptor {
            name: "Counter".to_string(),
            props_type: "CounterProps".to_string(),
            strategy: HydrationStrategy::Eager,
            import_path: "/islands/counter.js".to_string(),
            ssr_capable: true,
        });

        assert!(registry.is_registered("Counter"));
        assert!(!registry.is_registered("Unknown"));
    }

    #[test]
    fn test_instance_creation() {
        let mut registry = IslandRegistry::new();
        registry.register_default("Button");

        let instance = registry.create_instance(
            "Button",
            serde_json::json!({"label": "Click me"}),
            "<button>Click me</button>".to_string(),
        );

        assert!(instance.is_some());
        let instance = instance.unwrap();
        assert_eq!(instance.name, "Button");
        assert!(instance.props.contains("Click me"));
    }

    #[test]
    fn test_manifest_generation() {
        let mut registry = IslandRegistry::new();
        registry.register_default("TodoList");

        registry.create_instance(
            "TodoList",
            serde_json::json!({"items": ["a", "b"]}),
            "<ul><li>a</li><li>b</li></ul>".to_string(),
        );

        let manifest = registry.generate_manifest();
        assert_eq!(manifest.islands.len(), 1);
        assert_eq!(manifest.islands[0].name, "TodoList");
    }
}
