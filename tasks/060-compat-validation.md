# Task 060: Ink Compatibility Validation

## Goal
Warn users when they use unsupported or partially-supported Ink props.

## Problem
Users have no feedback when they use Ink props that TuiBridge:
1. Silently ignores
2. Partially supports (different behavior than Deno/Ink)

This causes visual differences that are hard to debug.

## Solution
Add a compatibility validation layer:

### 1. Create `src/compat.rs`

```rust
//! Ink Compatibility Validation

/// All Ink props supported by TuiBridge (Box components)
pub static SUPPORTED_BOX_PROPS: &[&str] = &[
    "flexDirection", "alignItems", "justifyContent", "flexWrap",
    "flexGrow", "flexShrink", "flexBasis",
    "margin", "marginTop", "marginBottom", "marginLeft", "marginRight",
    "marginX", "marginY",
    "padding", "paddingTop", "paddingBottom", "paddingLeft", "paddingRight",
    "paddingX", "paddingY",
    "borderStyle", "borderColor", "borderDimColor",
    "borderTop", "borderBottom", "borderLeft", "borderRight",
    "width", "height", "minWidth", "maxWidth", "minHeight", "maxHeight",
    "position", "display",
    "children",
];

/// All Ink props supported by TuiBridge (Text components)
pub static SUPPORTED_TEXT_PROPS: &[&str] = &[
    "color", "backgroundColor",
    "bold", "dimColor", "dim", "italic",
    "strikethrough", "underline", "inverse",
    "transform", "textWrap",
    "children",
];

/// Props with partial support
pub static PARTIAL_PROPS: &[&str] = &[
    "textWrap",  // "scroll" falls back to "wrap"
    "borderDimColor", // DIM modifier, not separate color
];

/// TextWrap enum
pub enum TextWrap {
    Wrap,     // Default
    Truncate, // Cut at width
    Ellipsis, // Show ... (approximated)
    Scroll,   // Falls back to Wrap
}

impl TextWrap {
    pub fn from_str(s: &str) -> Self {
        match s {
            "wrap" => TextWrap::Wrap,
            "truncate" => TextWrap::Truncate,
            "ellipsis" => TextWrap::Ellipsis,
            "scroll" => TextWrap::Scroll,
            _ => TextWrap::Wrap,
        }
    }
}

/// Validate props and return unsupported ones
pub fn validate_props(props: &HashSet<String>, component: &str) -> Vec<String> {
    let supported = match component {
        "ink-box" => SUPPORTED_BOX_PROPS,
        "ink-text" => SUPPORTED_TEXT_PROPS,
        _ => &[],
    };
    props.iter()
        .filter(|p| !supported.contains(&p.as_str()))
        .cloned()
        .collect()
}
```

### 2. Integrate in `bridge.rs::commit_update()`

After parsing props JSON, before applying them:

```rust
if is_debug_mode() {
    let unsupported = compat::validate_props(&props.keys().cloned().collect(), tag);
    for prop in unsupported {
        tracing::warn!("Unsupported prop '{}' on <{}>", prop, tag);
    }
}
```

### 3. Add `--debug` flag + `TUIBRIDGE_DEBUG` env var

```rust
// In main.rs
static DEBUG_MODE: AtomicBool = AtomicBool::new(false);

fn is_debug_mode() -> bool {
    DEBUG_MODE.load(Relaxed)
}

// Parse --debug flag and TUIBRIDGE_DEBUG env var
```

### 4. Log partial support warnings

```rust
// For textWrap="scroll"
tracing::warn!("textWrap='scroll' not fully supported, using wrap");

// For borderDimColor
tracing::debug!("borderDimColor uses DIM modifier, not separate color");
```

## Status
**Done** — Implemented in `src/compat.rs` and integrated in `bridge.rs`

## Implementation

### `src/compat.rs`
- `SUPPORTED_BOX_PROPS` — full list of supported Box props
- `SUPPORTED_TEXT_PROPS` — full list of supported Text props
- `PARTIAL_PROPS` — props with limited behavior
- `TextWrap` enum — wrap mode parsing
- `validate_box_props()` / `validate_text_props()` — validation
- `warn_unsupported_props()` — debug-only warnings

### `src/main.rs`
- `DEBUG_MODE` static + `is_debug_mode()` function
- `--debug` flag + `TUIBRIDGE_DEBUG` env var

### `src/bridge.rs::commit_update()`
- Validates props against component type in debug mode
- Logs warnings for unsupported props

### `src/main.rs::render_node()`
- `textWrap` prop handled: wrap, truncate, ellipsis, scroll (scroll falls back to wrap)

## Acceptance Criteria
- [x] `src/compat.rs` created with prop lists
- [x] Unsupported props logged as warnings when `--debug` enabled
- [x] Partial props (textWrap, borderDimColor) logged with behavior note
- [x] `--debug` flag and `TUIBRIDGE_DEBUG` env var work
- [x] No performance impact when debug mode is off

## Dependencies
- None

## SPEC Reference
§7 Performance (debug mode off = zero overhead)
