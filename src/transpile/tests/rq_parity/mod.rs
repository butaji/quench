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

// ink-react-advanced: useReducer, useContext, memo, forwardRef
ink_example_test!(
    test_ink_react_advanced,
    "ink-react-advanced",
    [
        "React Hooks Demo",
        "Theme: cyan",
        "Initial: 5, step: 2",
        "Value: 7",
        "After 2 increments: 9",
        "useReducer, useContext, memo, forwardRef all work."
    ]
);

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

ink_example_test!(
    test_ink_inline_type_import,
    "ink-inline-type-import",
    [
        "Inline Type Import Demo",
        "User: Alice (30)",
        "Status: active",
        "Alt: Bob (25)",
        "(type imports erased)"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Utility examples (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_switch, "ink-switch");

// ═══════════════════════════════════════════════════════════════════
// Operators & expressions (smoke tests)
// ═══════════════════════════════════════════════════════════════════

ink_example_smoke_test!(test_ink_nullish_optional, "ink-nullish-optional");
ink_example_smoke_test!(test_ink_generator, "ink-generator");
ink_example_smoke_test!(test_ink_proxy, "ink-proxy");
ink_example_smoke_test!(test_ink_weakref, "ink-weakref");
ink_example_smoke_test!(test_ink_string_modern, "ink-string-modern");
ink_example_test!(
    test_ink_string_search,
    "ink-string-search",
    [
        "Starts with Hello: yes",
        "Ends with World!: yes",
        "Includes TypeScript: yes",
        "Includes Rust: no",
        "Repeated: =-==-==-==-==-=",
        "Padded: [    hi]",
    ]
);
ink_example_smoke_test!(test_ink_promise_advanced, "ink-promise-advanced");
ink_example_smoke_test!(test_ink_this_parameter, "ink-this-parameter");
ink_example_smoke_test!(test_ink_unique_symbol, "ink-unique-symbol");
ink_example_smoke_test!(test_ink_react_children, "ink-react-children");

// arguments object - legacy but still used in some codebases
ink_example_test!(
    test_ink_arguments,
    "ink-arguments",
    [
        "Arguments Object Demo",
        "sumAll(1, 2, 3): 6",
        "sumAll(10, 20, 30, 40, 50): 150",
        "logArgs('a', 'b', 'c'): a, b, c",
        "maxOf(3, 1, 4, 1, 5, 9, 2, 6): 9",
        "toArray('x', 'y', 'z'): x, y, z"
    ]
);

ink_example_test!(
    test_ink_regexp_advanced,
    "ink-regexp-advanced",
    [
        "RegExp Advanced Demo",
        "matchCount: 2",
        "firstMatch: Hello",
        "hasNumber: true",
        "emailUser: user",
        "pascalCase: helloWorldTest",
        "year: 2024"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// TypeScript declaration patterns (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_function_overloads,
    "ink-function-overloads",
    [
        "String: HELLO",
        "Number: Number: 42",
        "Class string: WORLD",
        "Class number: Number: 99"
    ]
);

ink_example_test!(
    test_ink_namespace_declare,
    "ink-namespace-declare",
    [
        "App: MyApp",
        "Version: 1.0.0",
        "Build #: 42",
        "Theme: default"
    ]
);

ink_example_test!(
    test_ink_override_implements,
    "ink-override-implements",
    [
        "FancyWidget",
        "FancyWidget (decorated)",
        "[fancy]",
        "SimpleWidget",
        "base",
        "Count: 2"
    ]
);

ink_example_test!(
    test_ink_abstract_class,
    "ink-abstract-class",
    [
        "Abstract Class Demo",
        "Widget: Text",
        "Hello, World!",
        "Type: text",
        "Widget: Number",
        "#42",
        "Type: number"
    ]
);

ink_example_test!(
    test_ink_class_expression,
    "ink-class-expression",
    [
        "Class Expression Demo",
        "Counter initial: 0",
        "After 2 increments: 2",
        "Person: Hello, I'm Alice and I'm 30",
        "Model: Model #1: Widget",
        "Singleton value: 42",
        "Calc version: 1.0",
        "Calc value: 15"
    ]
);

ink_example_test!(
    test_ink_new_target,
    "ink-new-target",
    [
        "new.target Demo",
        "Circle \"Circle\" with radius 1",
        "Circle area: 78.54",
        "Square area: 16",
        "instanceof Shape: true",
        "instanceof Circle: true"
    ]
);

ink_example_test!(
    test_ink_reflect_api,
    "ink-reflect-api",
    [
        "Reflect API Demo",
        "Reflect.get(obj, 'name'): App",
        "Reflect.has(obj, 'name'): true",
        "Reflect.has(obj, 'id'): false",
        "Reflect.ownKeys(obj): name, version, author",
        "Reflect.deleteProperty(testObj, 'y'): true"
    ]
);

ink_example_test!(
    test_ink_template_literal_types,
    "ink-template-literal-types",
    [
        "Template Literal Types Demo",
        "bgRed: bg-red",
        "onClick: onclick",
        "marginTop: margin-top",
        "path1: /api/v1/users",
        "dataColor: data-color"
    ]
);

ink_example_test!(
    test_ink_infer_conditional,
    "ink-infer-conditional",
    [
        "infer in Conditional Types Demo",
        "ReturnType extraction:",
        "greeting: Hello",
        "age: 30",
        "user: {",
        "first: first-element"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// TypeScript utility types (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_utility_types,
    "ink-utility-types",
    [
        "TypeScript Utility Types Demo",
        "PartialUser.name: Alice",
        "UserPreview: Carol, 30",
        "statusLabels.done: Complete",
        "GreetParams: [World, 42]",
        "ReturnType: Hello, World!"
    ]
);

ink_example_test!(
    test_ink_discriminated_unions,
    "ink-discriminated-unions",
    [
        "Discriminated Unions Demo",
        "Click at (10, 20)",
        "Key: Enter",
        "circle: 78.54",
        "rect: 12.00",
        "div(10, 2): 5",
        "success: data=42"
    ]
);

ink_example_test!(
    test_ink_mapped_types,
    "ink-mapped-types",
    [
        "Mapped Types Demo",
        "name: Alice",
        "age: 30",
        "active: true",
        "name, age, active",
        "admin: read, write, delete"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// TypeScript as const & literal types (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_as_const,
    "ink-as-const",
    [
        "as const, Literal Types & Tuples Demo",
        "Colors[0]: red",
        "Config.title: My App",
        "Direction: north",
        "HTTP OK: 200",
        "Origin: (0, 0)",
        "Current status: loading"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// TypeScript index signatures & intersections (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_index_intersection,
    "ink-index-intersection",
    [
        "Index Signatures & Intersection Types Demo",
        "Name: Widget",
        "ABC.a: hello",
        "Dict entries:",
        "Widget.size (index): 100",
        "Meta entries:"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// TypeScript global/module augmentation (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_global_augmentation,
    "ink-global-augmentation",
    [
        "TypeScript Augmentation",
        "Build: development",
        "Global/Module declarations erased"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// TypeScript unknown, never, type guards (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_unknown_never,
    "ink-unknown-never",
    [
        "unknown, never & Type Guards Demo",
        "Status: Idle",
        "str: \"HELLO\"",
        "num: 42.00",
        "pet: cat",
        "string: 4"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// TypeScript asserts predicate (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_asserts_predicate,
    "ink-asserts-predicate",
    [
        "asserts Type Predicate Demo",
        "asserts value is Type narrows type after check",
        "formatUpper(\"hello world\") = HELLO WORLD",
        "double(42) = 84",
        "safeLength(\"test\") = 4",
        "safeLength([1,2,3]) = 3"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// TypeScript import() type syntax (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_import_types,
    "ink-import-types",
    [
        "import() Type Syntax Demo",
        "type T = import(\"./module\").Type",
        "name: Alice",
        "age: 30",
        "id: prod-123",
        "price: $99.99",
        "current: active"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// React 18 useInsertionEffect (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_use_insertion_effect,
    "ink-use-insertion-effect",
    [
        "useInsertionEffect Demo",
        "React 18 hook for style injection",
        "Insertion effect status:",
        "Effect ordering",
        "useInsertionEffect (first)"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Render props pattern (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_render_props,
    "ink-render-props",
    [
        "Render Props Pattern Demo",
        "Component receives function to control rendering",
        "Position: (42, 13)",
        "List with render props",
        "1. React",
        "Alpha: 100"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Higher-Order Components (HOC) pattern (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_hoc,
    "ink-hoc",
    [
        "Higher-Order Components (HOC) Demo",
        "Functions that enhance components",
        "withLoading:",
        "withCounter",
        "withBorder:",
        "withColor"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Symbol.iterator and custom iterables (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_symbol_iterator,
    "ink-symbol-iterator",
    [
        "Symbol.iterator & Custom Iterables Demo",
        "Defining custom iteration behavior",
        "Range(1,5) via for...of: 1, 2, 3, 4, 5",
        "iterateString(\"hello\"): h, e, l, l, o",
        "Custom PairList"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// key prop in lists and fragments (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_key_prop,
    "ink-key-prop",
    [
        "key Prop Demo",
        "key helps React identify changed items",
        "List with stable key (id)",
        "Nested lists",
        "List with index key"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Callback refs and useRef (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_ref_callback,
    "ink-ref-callback",
    [
        "useRef & Callback Refs Demo",
        "Refs for mutable values and imperative operations",
        "Count (via ref): 0",
        "Callback ref pattern",
        "Multiple refs of different types"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// in operator (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_in_operator,
    "ink-in-operator",
    [
        "in Operator Demo",
        "Checking property existence",
        "'name' in user: true",
        "'missing' in user: false",
        "0 in ['a','b','c']: true"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Modern array methods (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_array_modern,
    "ink-array-modern",
    [
        "Modern Array Methods Demo",
        "flat(1): [1, 2, 3, 4,5]",
        "flatMap: [hello, 5, world, 5]",
        "at(-1): 50",
        "toSorted: [1, 1, 2, 3, 4, 5, 6, 9]",
        "includes(20): true",
        "findLast < 8: 6"
    ]
);

ink_example_test!(
    test_ink_array_immutable,
    "ink-array-immutable",
    [
        "Immutable Array Methods Demo",
        "ES2023 toSpliced and with",
        "Original: 1, 2, 3, 4, 5",
        "Spliced: 1, a, b, 4, 5",
        "Replaced: 1, 2, X, 4, 5",
        "Unchanged: 1, 2, 3, 4, 5"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Modern object methods (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_object_modern,
    "ink-object-modern",
    [
        "Modern Object Methods Demo",
        "fromEntries.x: 10",
        "hasOwn('x'): true",
        "hasOwn('z'): false",
        "Object.is(NaN, NaN): true",
        "create.greeting: hello"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// React useImperativeHandle (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_use_imperative_handle,
    "ink-use-imperative-handle",
    [
        "useImperativeHandle & forwardRef Demo",
        "Count: 0",
        "Timer: 0 (stopped)",
        "Test component",
        "Counter ref: null"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// React useSyncExternalStore & useDeferredValue (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_use_sync_external_store,
    "ink-use-sync-external-store",
    [
        "useSyncExternalStore & useDeferredValue Demo",
        "Terminal: 80x24",
        "Counter: 0",
        "Deferred: initial text",
        "Apple",
        "Carrot"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// React createRef & useDebugValue (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_react_refs_debug,
    "ink-react-refs-debug",
    [
        "Count: 0",
        "createRef + useDebugValue exercised"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// JSON API (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_json_api,
    "ink-json-api",
    [
        "JSON.stringify & JSON.parse Demo",
        "Original: app=MyApp",
        "Reparsed: true",
        "Selective:",
        "Parsed array:"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Date, Math, Intl (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_date_math,
    "ink-date-math",
    [
        "=== Date ===",
        "toISOString: 2024-01-15T19:30:00.000Z",
        "getFullYear: 2024",
        "getMonth: 0",
        "getDate: 15",
        "=== Math ===",
        "Math.PI: 3.14",
        "Math.E: 2.71",
        "Math.abs(-42): 42",
        "Math.floor(4.7): 4",
        "Math.ceil(4.2): 5",
        "Math.round(4.5): 5",
        "Math.max(1,5,3,9,2): 9",
        "Math.min(1,5,3,9,2): 1"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Function.bind, call, apply (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_function_bind,
    "ink-function-bind",
    [
        "Bind: Hey, World!",
        "Call: Hello, World!",
        "Apply: Hi, World!",
        "Partial: 6"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Object meta methods (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_object_meta,
    "ink-object-meta",
    [
        "Object Meta-Methods",
        "create, defineProperty, freeze, seal, assign",
        "prototype.greet: Hello from prototype",
        "readonlyProp: read only",
        "isFrozen: true",
        "isSealed: true",
        "assigned: a=10, b=2, c=3"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// queueMicrotask (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_queue_microtask,
    "ink-queue-microtask",
    [
        "queueMicrotask Demo",
        "Synchronous code (module evaluation order)",
        "1. sync-start",
        "2. sync-middle",
        "3. sync-end"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Error.cause (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_error_cause,
    "ink-error-cause",
    [
        "Error.cause Demo",
        "ES2022 error chaining",
        "has cause: true",
        "cause message: Connection refused",
        "has cause: false",
        "AggregateError with cause",
        "Custom error class"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Class static blocks (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_static_block,
    "ink-static-block",
    [
        "Class Static Blocks Demo",
        "ES2022 static initialization",
        "DatabaseConfig static block",
        "ApiConfig static block",
        "ServiceRegistry static block",
        "host: localhost",
        "initialized: true",
        "services: 2"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Private methods and `in` operator for private fields (ES2022)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_private_methods,
    "ink-private-methods",
    [
        "Private Methods Demo",
        "Counter value: 3",
        "Counter has #count: yes",
        "Other has #count: no",
        "Stack size: 3",
        "Push overflow: false"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Namespace re-export (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_namespace_reexport,
    "ink-namespace-reexport",
    [
        "Namespace Re-export Demo",
        "2 + 3 = 5",
        "4 * 5 = 20",
        "PI = 3.14"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Parameter properties (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_parameter_properties,
    "ink-parameter-properties",
    [
        "Parameter Properties Demo",
        "Alice (30) [u-123]",
        "Name: Alice",
        "Age: 30",
        "Role: user"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Console methods example
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_console_methods,
    "ink-console-methods",
    [
        "Console Methods Demo",
        "Exercised:",
        "  - console.log",
        "  - console.info",
        "  - console.warn",
        "  - console.error",
        "  - console.time / timeEnd",
        "  - console.table"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// URI encoding example
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_uri_encoding,
    "ink-uri-encoding",
    [
        "URI Encoding Demo",
        "Original: hello world & foo=bar",
        "Encoded: hello%20world%20%26%20foo%3Dbar",
        "Decoded: hello world & foo=bar",
        "URI: https://example.com/path?query=hello%20world"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Error subclasses (tests with expected output)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_error_subclasses,
    "ink-error-subclasses",
    [
        "Error Subclasses Demo",
        "Age 30: valid",
        "RangeError: Age must be between 0 and 150",
        "TypeError: Age must be a number",
        "ReferenceError: missing is undefined"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Throw expression example
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_throw_expression,
    "ink-throw-expression",
    [
        "Throw Expression Demo",
        "Hello World",
        "assertDefined with throw IIFE works."
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Catch type annotation example
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_type_annotation_catch,
    "ink-type-annotation-catch",
    [
        "Catch Type Annotation Demo",
        "Something went wrong",
        "Caught: string error"
    ]
);

// ═══════════════════════════════════════════════════════════════════
// Iterator helpers example (TC39 Stage 3)
// ═══════════════════════════════════════════════════════════════════

ink_example_test!(
    test_ink_iterator_helpers,
    "ink-iterator-helpers",
    [
        "Iterator Helpers Demo",
        "map*2 filter>10 take(3): 12, 14, 16",
        "drop(2) reduce sum(3..5): 12",
        "filter even take(5): 2, 4, 6, 8, 10"
    ]
);
