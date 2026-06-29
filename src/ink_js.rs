//! Ink API exposed to JavaScript via the custom JS runtime
//!
//! The full reconciler runtime (hooks, render, reconciliation) lives in
//! src/runtime.js, which is loaded after register().
//!
//! This module provides the Ink tag constants and registration helpers.

use quench_runtime::Context;

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

/// Register all Ink API globals in the custom JS context.
///
/// The full render/hooks implementation is loaded from runtime.js after
/// this call. This function establishes the constants and namespace so
/// that simple-hello.js (and other plain-element scripts) work even if
/// runtime.js is not loaded.
pub fn register(ctx: &mut Context) {
    // Component tags
    ctx.set_global("Box".to_string(), quench_runtime::Value::String(BOX.to_string()));
    ctx.set_global("Text".to_string(), quench_runtime::Value::String(TEXT.to_string()));
    ctx.set_global("Static".to_string(), quench_runtime::Value::String(STATIC.to_string()));
    ctx.set_global("Newline".to_string(), quench_runtime::Value::String(NEWLINE.to_string()));
    ctx.set_global("Spacer".to_string(), quench_runtime::Value::String(SPACER.to_string()));

    // Ink namespace for compatibility
    let ink_ns = quench_runtime::Object::new(quench_runtime::ObjectKind::Ordinary);
    let ink_ns = std::rc::Rc::new(std::cell::RefCell::new(ink_ns));
    ink_ns.borrow_mut().set("Box", quench_runtime::Value::String(BOX.to_string()));
    ink_ns.borrow_mut().set("Text", quench_runtime::Value::String(TEXT.to_string()));
    ink_ns.borrow_mut().set("Static", quench_runtime::Value::String(STATIC.to_string()));
    ink_ns.borrow_mut().set("Newline", quench_runtime::Value::String(NEWLINE.to_string()));
    ink_ns.borrow_mut().set("Spacer", quench_runtime::Value::String(SPACER.to_string()));

    ctx.set_global("ink".to_string(), quench_runtime::Value::Object(ink_ns));
}
