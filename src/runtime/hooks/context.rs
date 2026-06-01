//! Context hooks

use std::sync::Arc;

/// Context value container
#[allow(dead_code)]
pub struct Context<T: Clone + Send + Sync + 'static> {
    value: Arc<T>,
}

impl<T: Clone + Send + Sync + 'static> Context<T> {
    /// Create a new context with a default value
    #[allow(dead_code)]
    pub fn new(default: T) -> Self {
        Context {
            value: Arc::new(default),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> Clone for Context<T> {
    fn clone(&self) -> Self {
        Context {
            value: self.value.clone(),
        }
    }
}

/// create_context - creates a context with a default value
#[allow(dead_code)]
pub fn create_context<T: Clone + Send + Sync + 'static>(default: T) -> Context<T> {
    Context::new(default)
}

/// use_context hook - retrieves value from context
///
/// Returns the context value, or panics if context not found.
/// In SSR without provider tree, returns default value.
#[allow(dead_code)]
pub fn use_context<T: Clone + Send + Sync + 'static>(ctx: &Context<T>) -> T {
    // SSR context: return the stored value
    // In full implementation, would walk provider tree
    (*ctx.value).clone()
}
