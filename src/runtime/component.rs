//! Component system for runts
//!
//! Provides the runtime support for components, including hooks context,
//! component metadata, and rendering helpers.

use std::sync::Arc;
use parking_lot::RwLock;

/// Component metadata for registration
pub trait ComponentMeta: Send + Sync {
    /// Get component name
    fn name(&self) -> &'static str;
    
    /// Get props type name (for serialization)
    fn props_type(&self) -> Option<&'static str> {
        None
    }
}

/// Global component registry
pub struct ComponentRegistry {
    components: RwLock<Vec<Arc<dyn ComponentMeta>>>,
}

impl ComponentRegistry {
    pub fn new() -> Self {
        Self {
            components: RwLock::new(Vec::new()),
        }
    }
    
    /// Register a component
    pub fn register<M: ComponentMeta + 'static>(&self, meta: M) {
        self.components.write().push(Arc::new(meta));
    }
    
    /// Get all registered components
    pub fn components(&self) -> Vec<String> {
        self.components.read()
            .iter()
            .map(|m| m.name().to_string())
            .collect()
    }
}

impl Default for ComponentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global component registry instance - lazily initialized
pub struct LazyRegistry {
    registry: RwLock<Option<ComponentRegistry>>,
}

impl LazyRegistry {
    pub const fn new() -> Self {
        Self {
            registry: RwLock::new(None),
        }
    }
    
    pub fn get(&self) -> Arc<ComponentRegistry> {
        let mut guard = self.registry.write();
        if guard.is_none() {
            *guard = Some(ComponentRegistry::new());
        }
        
        // Clone the components vector
        let components = guard.as_ref().unwrap().components.read().clone();
        
        Arc::new(ComponentRegistry {
            components: RwLock::new(components)
        })
    }
}

static COMPONENT_REGISTRY: LazyRegistry = LazyRegistry::new();

/// Get the global component registry
pub fn component_registry() -> Arc<ComponentRegistry> {
    COMPONENT_REGISTRY.get()
}

/// Hook context for component rendering
#[derive(Default)]
pub struct HookContext {
    /// Current hook index
    hook_index: RwLock<usize>,
    
    /// Hook storage
    hooks: RwLock<Vec<Box<dyn std::any::Any + Send + Sync>>>,
}

impl HookContext {
    /// Get a hook at the current index and advance
    #[allow(clippy::boxed_local)]
    pub fn get_hook<T: 'static + Clone + Send + Sync>(&self, init: impl FnOnce() -> T) -> T {
        let mut hooks = self.hooks.write();
        let mut index = self.hook_index.write();
        
        if *index < hooks.len() {
            // Return existing hook
            let hook = hooks[*index].downcast_ref::<T>();
            let result = hook.cloned().unwrap_or_else(|| init());
            *index += 1;
            result
        } else {
            // Initialize new hook
            let hook = init();
            hooks.push(Box::new(hook.clone()));
            *index += 1;
            hook
        }
    }
    
    /// Reset hook index for new render
    pub fn reset(&self) {
        *self.hook_index.write() = 0;
    }
}

/// Component information for registration
#[derive(Debug, Clone)]
pub struct ComponentInfo {
    /// Component name
    pub name: String,
    
    /// Props type name (for serialization)
    pub props_type: Option<String>,
}

impl ComponentInfo {
    /// Create a new component info
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            props_type: None,
        }
    }
    
    /// Set the props type
    pub fn props_type(mut self, props_type: impl Into<String>) -> Self {
        self.props_type = Some(props_type.into());
        self
    }
}
