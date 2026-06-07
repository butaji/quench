//! Per-example parity tests for rquickjs dev path.
//!
//! Each test reads an example's tui/app.tsx, transpiles it to JS,
//! evaluates in rquickjs with the ink bridge, and asserts that
//! rendering produces some output (basic smoke test).

use crate::transpile::bundler::transpile_to_js_bundled;

/// Common test setup: read example, transpile, and evaluate in rquickjs context.
fn setup_and_render(name: &str) -> anyhow::Result<String> {
    let path = std::path::Path::new("examples").join(name).join("tui/app.tsx");
    let js = transpile_to_js_bundled(&path)?;
    eval_js_and_render(&js)
}

fn eval_js_and_render(js: &str) -> anyhow::Result<String> {
    let runtime = rquickjs::Runtime::new()?;
    let ctx = rquickjs::Context::full(&runtime)?;
    ctx.with(|ctx| {
        runts_ink::js_bridge::install(&ctx)?;
        ctx.eval::<rquickjs::Value, _>(js)?;
        let out: String = ctx.eval("runts_ink.render_to_string(__runts_default({}))")?;
        Ok(out)
    })
}

/// Assert that rendered output is non-empty.
fn assert_has_output(output: &str, example: &str) {
    assert!(
        !output.trim().is_empty(),
        "Example {} should produce non-empty output, got:\n{}",
        example,
        output
    );
}

/// Assert that rendered output contains expected substring.
fn assert_contains(haystack: &str, needle: &str, example: &str) {
    assert!(
        haystack.contains(needle),
        "Example {} output should contain '{}', but got:\n{}",
        example,
        needle,
        haystack
    );
}

macro_rules! ink_example_smoke_test {
    ($name:ident, $example:expr) => {
        #[test]
        fn $name() {
            let output = setup_and_render($example).unwrap_or_else(|e| {
                panic!("Failed to render {}: {}", $example, e)
            });
            assert_has_output(&output, $example);
        }
    };
}

macro_rules! ink_example_test {
    ($name:ident, $example:expr, [$($expected:expr),* $(,)?]) => {
        #[test]
        fn $name() {
            let output = setup_and_render($example).unwrap_or_else(|e| {
                panic!("Failed to render {}: {}", $example, e)
            });
            assert_has_output(&output, $example);
            $(
                assert_contains(&output, $expected, $example);
            )*
        }
    };
}

// ═══════════════════════════════════════════════════════════════════
// Core component examples (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_text, "ink-text-props");
ink_example_smoke_test!(test_ink_box, "ink-box");

// ═══════════════════════════════════════════════════════════════════
// Styling examples (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_text_styling, "ink-text-styling");
ink_example_smoke_test!(test_ink_all_text_styles, "ink-all-text-styles");

// ink-background-color: backgroundColor not fully implemented in bridge
#[test]
#[ignore]
fn test_ink_background_color() {
    let _output = setup_and_render("ink-background-color").unwrap_or_else(|e| {
        panic!("Failed to render ink-background-color: {}", e)
    });
}

ink_example_smoke_test!(test_ink_border_color, "ink-border-color");

// ink-bordered: uses render(<App />) directly, not supported in rquickjs path
#[test]
#[ignore]
fn test_ink_bordered() {
    let _output = setup_and_render("ink-bordered").unwrap_or_else(|e| {
        panic!("Failed to render ink-bordered: {}", e)
    });
}

ink_example_smoke_test!(test_ink_partial_border, "ink-partial-border");
ink_example_smoke_test!(test_ink_all_border_styles, "ink-all-border-styles");
ink_example_smoke_test!(test_ink_inverse, "ink-inverse");

// ═══════════════════════════════════════════════════════════════════
// Layout examples (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_layout, "ink-layout");
ink_example_smoke_test!(test_ink_aligned, "ink-aligned");
ink_example_smoke_test!(test_ink_margin, "ink-margin");
ink_example_smoke_test!(test_ink_padding, "ink-padding");
ink_example_smoke_test!(test_ink_gaps, "ink-gaps");
ink_example_smoke_test!(test_ink_wrap, "ink-wrap");
ink_example_smoke_test!(test_ink_spacer, "ink-spacer");
ink_example_smoke_test!(test_ink_newline, "ink-newline");
ink_example_smoke_test!(test_ink_nested_layouts, "ink-nested-layouts");
ink_example_smoke_test!(test_ink_relative, "ink-relative");
ink_example_smoke_test!(test_ink_absolute, "ink-absolute");

// ═══════════════════════════════════════════════════════════════════
// Advanced layout (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_display, "ink-display");
ink_example_smoke_test!(test_ink_overflow, "ink-overflow");
ink_example_smoke_test!(test_ink_flex_reverse, "ink-flex-reverse");
ink_example_smoke_test!(test_ink_flex_basis, "ink-flex-basis");
ink_example_smoke_test!(test_ink_align_self, "ink-align-self");
ink_example_smoke_test!(test_ink_dimensions, "ink-dimensions");
ink_example_smoke_test!(test_ink_min_max_size, "ink-min-max-size");
ink_example_smoke_test!(test_ink_z_index, "ink-z-index");
ink_example_smoke_test!(test_ink_justify_space, "ink-justify-space");

// ═══════════════════════════════════════════════════════════════════
// Static content (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_static, "ink-static");
ink_example_smoke_test!(test_ink_static_color, "ink-static-color");
ink_example_smoke_test!(test_ink_transform, "ink-transform");
ink_example_smoke_test!(test_ink_raw, "ink-raw");

// ═══════════════════════════════════════════════════════════════════
// Conditional rendering (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_conditional, "ink-conditional");
ink_example_smoke_test!(test_ink_conditional_rendering, "ink-conditional-rendering");
ink_example_smoke_test!(test_ink_dynamic, "ink-dynamic");
ink_example_smoke_test!(test_ink_dynamic_children, "ink-dynamic-children");

// ═══════════════════════════════════════════════════════════════════
// Fragment examples (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_fragment, "ink-fragment");
ink_example_smoke_test!(test_ink_fragment_advanced, "ink-fragment-advanced");

// ═══════════════════════════════════════════════════════════════════
// Component examples (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_list, "ink-list");
ink_example_smoke_test!(test_ink_list_advanced, "ink-list-advanced");
ink_example_smoke_test!(test_ink_progress, "ink-progress");
ink_example_smoke_test!(test_ink_progress_bar, "ink-progress-bar");
ink_example_smoke_test!(test_ink_table, "ink-table");
ink_example_smoke_test!(test_ink_table_advanced, "ink-table-advanced");
ink_example_smoke_test!(test_ink_menu, "ink-menu");
ink_example_smoke_test!(test_ink_menu_advanced, "ink-menu-advanced");
ink_example_smoke_test!(test_ink_split_pane, "ink-split-pane");

// ═══════════════════════════════════════════════════════════════════
// Form components (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_form_checkbox, "ink-form-checkbox");
ink_example_smoke_test!(test_ink_form_switch, "ink-form-switch");
ink_example_smoke_test!(test_ink_form_layout, "ink-form-layout");

// ═══════════════════════════════════════════════════════════════════
// Basic hooks (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_hooks, "ink-hooks");
ink_example_smoke_test!(test_ink_app_hook, "ink-app-hook");
ink_example_smoke_test!(test_ink_use_app, "ink-use-app");

// ═══════════════════════════════════════════════════════════════════
// Input hooks (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_input, "ink-input");
ink_example_smoke_test!(test_ink_input_hook, "ink-input-hook");
ink_example_smoke_test!(test_ink_uncontrolled_input, "ink-uncontrolled-input");

// ═══════════════════════════════════════════════════════════════════
// Interactive hooks (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_counter, "ink-counter");
ink_example_smoke_test!(test_ink_key_events, "ink-key-events");
ink_example_smoke_test!(test_ink_mouse_events, "ink-mouse-events");
ink_example_smoke_test!(test_ink_cursor, "ink-cursor");
ink_example_smoke_test!(test_ink_stdin, "ink-stdin");
ink_example_smoke_test!(test_ink_stdin_advanced, "ink-stdin-advanced");
ink_example_smoke_test!(test_ink_stdout, "ink-stdout");
ink_example_smoke_test!(test_ink_stderr, "ink-stderr");
ink_example_smoke_test!(test_ink_window_size, "ink-window-size");
ink_example_smoke_test!(test_ink_enter_submit, "ink-enter-submit");

// ═══════════════════════════════════════════════════════════════════
// Focus management (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_focus, "ink-focus");
ink_example_smoke_test!(test_ink_focus_manager, "ink-focus-manager");
ink_example_smoke_test!(test_ink_focus_next, "ink-focus-next");
ink_example_smoke_test!(test_ink_focus_cycle, "ink-focus-cycle");

// ═══════════════════════════════════════════════════════════════════
// Advanced hooks (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_animation, "ink-animation");
ink_example_smoke_test!(test_ink_measure, "ink-measure");

// ═══════════════════════════════════════════════════════════════════
// Context (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_context, "ink-context");
ink_example_smoke_test!(test_ink_context_advanced, "ink-context-advanced");

// ═══════════════════════════════════════════════════════════════════
// React hooks (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_use_effect, "ink-use-effect");
ink_example_smoke_test!(test_ink_use_memo, "ink-use-memo");
ink_example_smoke_test!(test_ink_use_callback, "ink-use-callback");
ink_example_smoke_test!(test_ink_rerender, "ink-rerender");

// ═══════════════════════════════════════════════════════════════════
// Combined examples (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_combined_hooks, "ink-combined-hooks");
ink_example_smoke_test!(test_ink_custom_render, "ink-custom-render");
ink_example_smoke_test!(test_ink_status_bar, "ink-status-bar");
ink_example_smoke_test!(test_ink_todo, "ink-todo");
ink_example_smoke_test!(test_ink_multi_select, "ink-multi-select");

// ═══════════════════════════════════════════════════════════════════
// Import examples (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_import, "ink-import");
ink_example_smoke_test!(test_ink_multiple_colors, "ink-multiple-colors");

// ═══════════════════════════════════════════════════════════════════
// Utility examples (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_switch, "ink-switch");

// ═══════════════════════════════════════════════════════════════════
// Operators & expressions (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_nullish_optional, "ink-nullish-optional");
