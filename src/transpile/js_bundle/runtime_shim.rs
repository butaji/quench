//! Runtime shim globals for the rquickjs dev path.
//!
//! `PRE_SHIM` defines arrays that must exist before user code
//! evaluates, because module-level `render()` calls may trigger
//! hooks that push into them.
//!
//! `POST_SHIM` defines `__runts_render_with_effects` and a
//! minimal `process` fallback, placed after all bundled modules.

/// Globals defined before user code.
pub const PRE_SHIM: &str = r#"
var __runts_effects = [];
var __runts_has_effects = false;
var __runts_layout_effects = [];
var __runts_has_layout_effects = false;
"#;

/// Render helper and process fallback placed after user code.
pub const POST_SHIM: &str = r#"
// Ensure a minimal process stub exists only if the bridge hasn't installed one.
if (typeof process === 'undefined') {
    var process = { exit: function(code) { __runts_exit = true; __runts_exit_code = code || 0; } };
}
function __runts_render_with_effects(props) {
    var vnode;

    // Reset per-render state so context values don't bleed between renders.
    if (typeof __ctxStack !== 'undefined') __ctxStack.length = 0;

    // Render the component first
    if (typeof __runts_default === 'function') {
        vnode = __runts_default(props || {});
    } else if (typeof __runts_app !== 'undefined') {
        vnode = __runts_app;
    } else {
        throw new Error('No app found: use export default or render(<App />)');
    }

    // Run layout effects (synchronous, after render, before paint)
    // Layout effects can trigger state changes, so we may need to re-render
    var guard = 0;
    while (__runts_has_layout_effects && guard < 10) {
        __runts_has_layout_effects = false;
        if (typeof __ctxStack !== 'undefined') __ctxStack.length = 0;
        var layoutEffects = __runts_layout_effects;
        __runts_layout_effects = [];
        for (var i = 0; i < layoutEffects.length; i++) {
            if (typeof layoutEffects[i] === 'function') layoutEffects[i]();
        }
        // Re-render after layout effects if state changed
        if (typeof __runts_default === 'function') {
            vnode = __runts_default(props || {});
        }
        guard++;
    }
    if (typeof __runts_default === 'function') {
        vnode = __runts_default(props || {});
    } else if (typeof __runts_app !== 'undefined') {
        vnode = __runts_app;
    } else {
        throw new Error('No app found: use export default or render(<App />)');
    }
    var guard = 0;
    while (__runts_has_effects && guard < 10) {
        __runts_has_effects = false;
        if (typeof __ctxStack !== 'undefined') __ctxStack.length = 0;
        var effects = __runts_effects;
        __runts_effects = [];
        for (var i = 0; i < effects.length; i++) {
            if (typeof effects[i] === 'function') effects[i]();
        }
        if (typeof __runts_default === 'function') {
            vnode = __runts_default(props || {});
        }
        guard++;
    }
    return vnode;
}"#;
