//! Ink API exposed to JavaScript via rquickjs
//!
//! The full reconciler runtime (hooks, render, reconciliation) lives in
//! src/runtime.js, which is loaded after register().
//!
//! This module provides native rquickjs bindings for Ink constants.
//! Functionality can be moved from runtime.js into native Rust here
//! incrementally as needed.

use rquickjs::{Ctx, Result as JsResult};

// ============================================================================
// Component Tags (Task 011)
// ============================================================================

pub const BOX: &str = "ink-box";
pub const TEXT: &str = "ink-text";
pub const STATIC: &str = "ink-static";
pub const NEWLINE: &str = "ink-newline";
pub const SPACER: &str = "ink-spacer";

// ============================================================================
// Module Registration (Task 009)
// ============================================================================

/// Register all Ink API globals in the QuickJS context.
///
/// The full render/hooks implementation is loaded from runtime.js after
/// this call.  This function establishes the constants and namespace so
/// that simple-hello.js (and other plain-element scripts) work even if
/// runtime.js is not loaded.
pub fn register<'js>(ctx: Ctx<'js>) -> JsResult<()> {
    let globals = ctx.globals();

    // Component tags
    globals.set("Box", BOX)?;
    globals.set("Text", TEXT)?;
    globals.set("Static", STATIC)?;
    globals.set("Newline", NEWLINE)?;
    globals.set("Spacer", SPACER)?;

    // Ink namespace for compatibility
    let ink_ns = rquickjs::Object::new(ctx)?;
    ink_ns.set("Box", BOX)?;
    ink_ns.set("Text", TEXT)?;
    ink_ns.set("Static", STATIC)?;
    ink_ns.set("Newline", NEWLINE)?;
    ink_ns.set("Spacer", SPACER)?;
    globals.set("ink", ink_ns)?;

    Ok(())
}
