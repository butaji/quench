//! Islands architecture for runts
//!
//! This module implements the islands architecture for partial hydration.
//! Islands are interactive components that are hydrated on the client side,
//! while the rest of the page is rendered server-side and static.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Hydration mode for islands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HydrationMode {
    /// Hydrate immediately on page load
    Eager,
    /// Hydrate when visible (intersection observer)
    #[default]
    Lazy,
    /// Hydrate when interacted with (click, focus, etc.)
    Interaction,
    /// Hydrate when dynamically added to DOM
    Visible,
}

impl HydrationMode {
    /// Parse from string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "eager" => Self::Eager,
            "lazy" => Self::Lazy,
            "interaction" => Self::Interaction,
            "visible" => Self::Visible,
            _ => Self::Lazy,
        }
    }

    #[allow(clippy::wrong_self_convention)]
    fn to_attribute(&self) -> &'static str {
        match self {
            Self::Eager => "eager",
            Self::Lazy => "lazy",
            Self::Interaction => "interaction",
            Self::Visible => "visible",
        }
    }
}

/// An island manifest entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandManifest {
    /// Unique identifier
    pub id: String,
    /// Component name (PascalCase)
    pub name: String,
    /// File path
    pub file: String,
    /// Props type name (for TypeScript)
    pub props_type: Option<String>,
    /// Hydration mode
    pub hydration: HydrationMode,
    /// Import path for client bundle
    pub import_path: String,
}

/// Props for an island instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandProps {
    /// Serialized props (JSON)
    pub data: serde_json::Value,
    /// Signal initial values (for fine-grained sync)
    #[serde(default)]
    pub signals: Option<HashMap<String, serde_json::Value>>,
}

impl IslandProps {
    /// Create new props from a serializable value
    pub fn new<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        Ok(Self {
            data: serde_json::to_value(value)?,
            signals: None,
        })
    }

    /// Create props with signal initial values
    pub fn with_signals<T: Serialize>(
        value: &T,
        signals: HashMap<String, serde_json::Value>,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            data: serde_json::to_value(value)?,
            signals: Some(signals),
        })
    }

    /// Deserialize props to a type
    #[allow(dead_code)]
    pub fn deserialize<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.data.clone())
    }
}

/// Island instance (server-side representation)
pub struct Island {
    /// Component name
    name: String,
    /// Props passed to the component
    props: IslandProps,
    /// Hydration mode
    hydration: HydrationMode,
    /// Server-rendered HTML placeholder
    placeholder_html: String,
    /// Unique instance ID
    id: String,
}

impl Island {
    /// Create a new island
    pub fn new(name: impl Into<String>, props: IslandProps) -> Self {
        Self {
            name: name.into(),
            props,
            hydration: HydrationMode::Lazy,
            placeholder_html: String::new(),
            id: Self::generate_id(),
        }
    }

    /// Set hydration mode
    #[must_use]
    pub fn with_hydration(mut self, mode: HydrationMode) -> Self {
        self.hydration = mode;
        self
    }

    /// Set placeholder HTML (from SSR)
    #[must_use]
    pub fn with_placeholder(mut self, html: String) -> Self {
        self.placeholder_html = html;
        self
    }

    /// Generate unique ID
    fn generate_id() -> String {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        format!("island-{:x}-{:x}", timestamp, counter)
    }

    /// Generate the island container HTML
    pub fn to_html(&self) -> String {
        let _props_json = serde_json::to_string(&self.props.data)
            .unwrap_or_else(|_| "{}".to_string());

        format!(
            r#"<div data-island="{}" data-id="{}" data-hydration="{}">{}</div>"#,
            self.name,
            self.id,
            self.hydration.to_attribute(),
            self.placeholder_html
        )
    }

    /// Generate the hydration data script tag
    pub fn hydration_script(&self) -> String {
        let props_json = serde_json::to_string(&self.props.data)
            .unwrap_or_else(|_| "{}".to_string());

        format!(
            r#"<script type="application/x-runts-island" id="island-data-{}">{}</script>"#,
            self.id,
            props_json
        )
    }

    /// Generate inline hydration configuration
    pub fn hydration_config(&self) -> String {
        format!(
            r#"data-island="{}" data-id="{}" data-hydration="{}""#,
            self.name,
            self.id,
            self.hydration.to_attribute()
        )
    }

    /// Get the island ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the island name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get hydration mode
    pub fn hydration_mode(&self) -> HydrationMode {
        self.hydration
    }

    /// Get props
    pub fn props(&self) -> &IslandProps {
        &self.props
    }
}

/// Island registry for tracking all islands
#[derive(Debug, Clone, Default)]
pub struct IslandRegistry {
    /// Registered islands
    islands: Arc<Vec<IslandManifest>>,
}

impl IslandRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            islands: Arc::new(Vec::new()),
        }
    }

    /// Register an island
    pub fn register(&mut self, manifest: IslandManifest) {
        Arc::make_mut(&mut self.islands).push(manifest);
    }

    /// Get all registered islands
    pub fn islands(&self) -> &[IslandManifest] {
        &self.islands
    }

    /// Find an island by name
    pub fn find(&self, name: &str) -> Option<&IslandManifest> {
        self.islands.iter().find(|i| i.name == name)
    }

    /// Find an island by ID
    pub fn find_by_id(&self, id: &str) -> Option<&IslandManifest> {
        self.islands.iter().find(|i| i.id == id)
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&*self.islands)
    }

    /// Serialize to pretty JSON
    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&*self.islands)
    }
}

/// Server-side island renderer
pub struct IslandRenderer {
    /// Registry of known islands
    registry: IslandRegistry,
    /// Renderer for components
    component_renderer: Box<dyn Fn(&str, &serde_json::Value) -> String + Send + Sync>,
}

impl IslandRenderer {
    /// Create a new renderer
    pub fn new() -> Self {
        Self {
            registry: IslandRegistry::new(),
            component_renderer: Box::new(|_name, _props| {
                "<!-- island placeholder -->".to_string()
            }),
        }
    }

    /// Create with registry
    pub fn with_registry(registry: IslandRegistry) -> Self {
        Self {
            registry,
            component_renderer: Box::new(|_name, _props| {
                "<!-- island placeholder -->".to_string()
            }),
        }
    }

    /// Set a custom component renderer
    pub fn with_renderer<F>(mut self, renderer: F) -> Self
    where
        F: Fn(&str, &serde_json::Value) -> String + 'static + Send + Sync,
    {
        self.component_renderer = Box::new(renderer);
        self
    }

    /// Render an island server-side
    pub fn render_island(&self, name: &str, props: &serde_json::Value) -> String {
        let html = (self.component_renderer)(name, props);

        let props_json = serde_json::to_string(props).unwrap_or_else(|_| "{}".to_string());

        format!(
            r#"<div data-island="{}" data-props="{}">{}</div>"#,
            name,
            Self::escape_attr(&props_json),
            html
        )
    }

    /// Render all islands found in HTML
    #[allow(dead_code)]
    pub fn render_islands(&self, html: &str) -> String {
        html.to_string()
    }

    /// Escape attribute values
    fn escape_attr(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    /// Get the registry
    pub fn registry(&self) -> &IslandRegistry {
        &self.registry
    }
}

impl Default for IslandRenderer {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Client-side hydration types
// =============================================================================

/// Client-side island hydration state
#[derive(Debug, Clone, PartialEq)]
pub enum HydrationState {
    /// Not yet hydrated
    Pending,
    /// Currently hydrating
    Hydrating,
    /// Successfully hydrated
    Hydrated,
    /// Hydration failed
    Error(String),
}

/// Client-side island instance
pub struct ClientIsland {
    /// Unique ID
    pub id: String,
    /// Component name
    pub name: String,
    /// Props
    pub props: serde_json::Value,
    /// Current state
    pub state: HydrationState,
}

impl ClientIsland {
    /// Create a new client island
    pub fn new(name: String, id: String, props: serde_json::Value) -> Self {
        Self {
            id,
            name,
            props,
            state: HydrationState::Pending,
        }
    }

    /// Start hydration
    pub async fn hydrate(&mut self) -> Result<(), String> {
        self.state = HydrationState::Hydrating;
        self.state = HydrationState::Hydrated;
        Ok(())
    }

    /// Get the hydration state
    pub fn state(&self) -> &HydrationState {
        &self.state
    }
}

// =============================================================================
// Hydration protocol
// =============================================================================

/// Hydration message (WebSocket or postMessage)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HydrationMessage {
    /// Request to hydrate an island
    Hydrate {
        id: String,
        name: String,
        props: serde_json::Value,
    },
    /// Island has been hydrated
    Hydrated {
        id: String,
    },
    /// Hydration failed
    HydrationError {
        id: String,
        error: String,
    },
    /// Signal update from server
    SignalUpdate {
        id: String,
        signal: String,
        value: serde_json::Value,
    },
    /// Request full reload
    Reload,
    /// Hot module update
    HmrUpdate {
        module: String,
        code: String,
    },
    /// State sync request
    SyncState {
        id: String,
        state: serde_json::Value,
    },
}

/// Client-side hydration manager
pub struct HydrationManager {
    /// Active islands
    islands: HashMap<String, ClientIsland>,
    /// Event handlers
    handlers: HashMap<String, Box<dyn Fn(String, serde_json::Value) + Send + Sync>>,
}

impl HydrationManager {
    /// Create a new manager
    pub fn new() -> Self {
        Self {
            islands: HashMap::new(),
            handlers: HashMap::new(),
        }
    }

    /// Register an island
    pub fn register(&mut self, name: String, id: String, props: serde_json::Value) {
        let island = ClientIsland::new(name, id.clone(), props);
        self.islands.insert(id, island);
    }

    /// Hydrate all pending islands
    pub async fn hydrate_all(&mut self) {
        for island in self.islands.values_mut() {
            if island.state == HydrationState::Pending {
                if let Err(e) = island.hydrate().await {
                    eprintln!("Failed to hydrate {}: {}", island.name, e);
                }
            }
        }
    }

    /// Handle an incoming message
    pub fn handle_message(&mut self, msg: HydrationMessage) {
        match msg {
            HydrationMessage::Hydrated { id } => {
                if let Some(island) = self.islands.get_mut(&id) {
                    island.state = HydrationState::Hydrated;
                }
            }
            HydrationMessage::HydrationError { id, error } => {
                if let Some(island) = self.islands.get_mut(&id) {
                    island.state = HydrationState::Error(error);
                }
            }
            HydrationMessage::SignalUpdate { id, signal, value } => {
                let key = format!("{}.{}", id, signal);
                if let Some(handler) = self.handlers.get(&key) {
                    handler(id, value);
                }
            }
            HydrationMessage::Reload | HydrationMessage::HmrUpdate { .. } | HydrationMessage::SyncState { .. } => {}
            HydrationMessage::Hydrate { .. } => {}
        }
    }
}

impl Default for HydrationManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Island Component Trait
// =============================================================================

/// Trait for island components
pub trait IslandComponent: Send + Sync {
    /// Get the component name
    fn name(&self) -> &str;

    /// Render the component server-side
    fn render_server(&self, props: &serde_json::Value) -> String;

    /// Get the client bundle path
    fn client_bundle(&self) -> &str;

    /// Get hydration mode
    fn hydration_mode(&self) -> HydrationMode {
        HydrationMode::Lazy
    }
}

/// Island component registry
pub struct IslandComponentRegistry {
    components: HashMap<String, Box<dyn IslandComponent>>,
}

impl IslandComponentRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
        }
    }

    /// Register a component
    pub fn register<C: IslandComponent + 'static>(&mut self, component: C) {
        let name = component.name().to_string();
        self.components.insert(name, Box::new(component));
    }

    /// Get a component by name
    pub fn get(&self, name: &str) -> Option<&dyn IslandComponent> {
        self.components.get(name).map(|b| b.as_ref())
    }

    /// Render an island by name
    pub fn render(&self, name: &str, props: &serde_json::Value) -> Option<String> {
        self.get(name).map(|c| c.render_server(props))
    }

    /// Check if a component exists
    pub fn contains(&self, name: &str) -> bool {
        self.components.contains_key(name)
    }

    /// Get all component names
    pub fn names(&self) -> Vec<&str> {
        self.components.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for IslandComponentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Static Island Rendering
// =============================================================================

/// Static island descriptor (for compile-time islands)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticIsland {
    /// Component name
    pub name: String,
    /// Module path
    pub module: String,
    /// Props type
    pub props_type: Option<String>,
    /// Hydration mode
    pub hydration: HydrationMode,
    /// Client bundle path
    pub bundle: Option<String>,
}

impl StaticIsland {
    /// Create a new static island descriptor
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            module: String::new(),
            props_type: None,
            hydration: HydrationMode::Lazy,
            bundle: None,
        }
    }

    /// Set the module path
    #[must_use]
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module = module.into();
        self
    }

    /// Set hydration mode
    #[must_use]
    pub fn with_hydration(mut self, mode: HydrationMode) -> Self {
        self.hydration = mode;
        self
    }

    /// Set client bundle path
    #[must_use]
    pub fn with_bundle(mut self, bundle: impl Into<String>) -> Self {
        self.bundle = Some(bundle.into());
        self
    }
}
