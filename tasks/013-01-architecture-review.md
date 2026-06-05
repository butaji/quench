# Task 013-01: Architecture Review - runts-ink

**Date:** 2026-06-05

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        User TypeScript                           │
│  examples/ink-counter/tui/app.tsx                                │
│  import { Box, Text } from 'ink';                               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        runts pipeline                           │
│                                                                  │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐   │
│  │  oxc_parser │ -> │     HIR     │ -> │  Type-directed   │   │
│  │  (Type AST) │    │ (High-level │    │    lowering     │   │
│  │             │    │   IR)       │    │                 │   │
│  └─────────────┘    └─────────────┘    └─────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┴───────────────────┐
          ▼                                       ▼
┌─────────────────────┐              ┌─────────────────────────┐
│   runts dev mode    │              │   runts build mode      │
│   (HIR runtime)     │              │   (Rust codegen)        │
│                     │              │                         │
│ ┌─────────────────┐ │              │ ┌─────────────────────┐ │
│ │  rquickjs       │ │              │ │ runts-ratatui       │ │
│ │  (QuickJS)      │ │              │ │ plugin::codegen    │ │
│ │                 │ │              │ │                     │ │
│ │  JS reconciler  │ │              │ │ Generates:          │ │
│ │  + Ink bridge   │ │              │ │  - Ratatui widgets  │ │
│ └─────────────────┘ │              │ │  - Yoga/Taffy layout│ │
│         │            │              │ │  - crossterm events│ │
│         ▼            │              │ └─────────────────────┘ │
│ ┌─────────────────┐ │              │           │              │
│ │ runts-ink      │ │              │           ▼              │
│ │ (Rust render)  │ │              │ ┌─────────────────────┐ │
│ │                │ │              │ │  Rust source        │ │
│ │  - flex_layout │ │              │ │  (generated .rs)    │ │
│ │  - ratatui     │ │              │ └─────────────────────┘ │
│ │  - crossterm   │ │              │           │              │
│ └─────────────────┘ │              │           ▼              │
└─────────────────────┘              │ ┌─────────────────────┐ │
                                     │ │  cargo build       │ │
                                     │ └─────────────────────┘ │
                                     │           │              │
                                     │           ▼              │
                                     │ ┌─────────────────────┐ │
                                     │ │  Native binary     │ │
                                     │ └─────────────────────┘ │
                                     └─────────────────────────┘
```

## Component Architecture

### 1. runts-ink Crate (`crates/runts-ink/`)

The core Ink-compatible components library:

| File | Purpose |
|------|---------|
| `lib.rs` | Public API exports |
| `components.rs` | Box, Text, Spacer, Newline, Static, Transform |
| `style.rs` | BorderStyle, Borders, Display, Overflow, Position |
| `events.rs` | InputEvent, Key, FocusId, MouseEvent, ResizeEvent |
| `props.rs` | Props handling for JS bridge |
| `vnode.rs` | VNode type for tree representation |
| `render.rs` | Main render loop (for Rust users) |
| `js_bridge.rs` | rquickjs FFI - exposes Rust API to JS |
| `flex_layout/` | Layout engine dispatcher (Taffy/Yoga) |

### 2. runts-ratatui Plugin (`crates/runts-ratatui/`)

The codegen plugin that compiles TSX to Rust:

| File | Purpose |
|------|---------|
| `lib.rs` | Plugin exports |
| `codegen.rs` | Main code generation (80KB+) |
| `plugin.rs` | Plugin interface implementation |
| `dev_jsx.rs` | Dev mode JSX handling |
| `plugin_test.rs` | Comprehensive tests |

### 3. Flex Layout Engines

Two layout backends supported (mutually exclusive):

#### Taffy (default)
- Pure Rust implementation
- Enabled via `features = ["taffy"]`
- `flex_layout/taffy.rs`

#### Yoga
- Facebook's C++ flexbox engine
- Enabled via `features = ["yoga"]`
- `flex_layout/yoga.rs`

## Key Data Types

### VNode (virtual node)
```rust
pub struct VNode(pub VNodeContent);

pub enum VNodeContent {
    Box(InkBox),
    Text(Text),
    Newline(Newline),
    Spacer(Spacer),
    Static(Static),
    Transform(Transform),
    Fragment(Vec<VNode>),
}
```

### Box Component
```rust
pub struct Box {
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub padding: PaddingRect,
    pub margin: MarginRect,
    pub border: BorderRect,
    pub align_items: AlignItems,
    pub justify_content: JustifyContent,
    pub children: Vec<VNode>,
    // ... more fields
}
```

### Text Component
```rust
pub struct Text {
    pub value: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub dim_color: bool,
    // ... more fields
}
```

## Rendering Pipeline

### HIR Runtime Path (dev mode)
```
1. Parse TSX -> HIR
2. Execute HIR in rquickjs
3. JS reconciler creates VNodes via js_bridge
4. Rust receives VNodes over mpsc channel
5. Compute Yoga/Taffy layout
6. Render to Ratatui
7. Poll crossterm events
8. Route events back to JS handlers
```

### Compile Path (build mode)
```
1. Parse TSX -> HIR
2. Type-directed lowering to Rust
3. Generate Ratatui code
4. cargo build
5. Execute native binary
6. Same rendering pipeline as above
```

## Example Structure

Each ink example has:
```
examples/ink-counter/
├── main.tsx              # Renders <App />
├── tui/
│   └── app.tsx           # Main component
├── deno.json             # Deno imports
├── runts.config.json     # Plugin config
└── target/               # Build artifacts
```

## Test Structure

### Unit Tests
- `tests/ink_parity_unified_tests.rs` - Example structure validation
- `tests/ink_harness_unit_tests.rs` - Harness function tests
- `tests/ink_parity_harness_unit_tests.rs` - Harness integration tests
- In-crate tests in each module

### Integration Tests
- `tests/ink_examples_snapshot_tests.rs` - Snapshot tests
- `tests/ink_comprehensive_tests.rs` - Full component tests

### Parity Harness
- `test_ink_parity_unified.sh` - Main test script

## Hook Support Status

| Hook | HIR Runtime | Compile | Notes |
|------|-------------|---------|-------|
| useState | ✅ Full | ✅ Full | Works everywhere |
| useEffect | ⚠️ Partial | ✅ Full | Timing differs |
| useInput | ❌ Static | ✅ Full | Not in HIR |
| useStdin | ❌ N/A | ❌ N/A | Not supported |
| useFocus | ❌ Static | ⚠️ Partial | Limited |
| useApp | ❌ Static | ✅ Full | exit() only |
| useCursor | ⚠️ Static | ✅ Full | Static positioning |

## Feature Parity Matrix

| Feature | deno | HIR | Compile |
|---------|------|-----|---------|
| Box layout | ✅ | ✅ | ✅ |
| Text styling | ✅ | ✅ | ✅ |
| Borders | ✅ | ✅ | ✅ |
| Flexbox | ✅ | ✅ | ✅ |
| Nested layouts | ✅ | ✅ | ✅ |
| Static examples | ✅ | ✅ | ✅ |
| Interactive | ✅ | ❌ | ✅ |

## Code Statistics

| Metric | Value |
|--------|-------|
| Total examples | 88 |
| Static examples | 70 |
| Hook examples | 18 |
| Test files | 14 |
| Unit tests | ~1100+ |
| Components | 6 |
| Style enums | 8 |
| Event types | 5 |

## Dependencies

### runts-ink
- ratatui 0.26
- crossterm 0.27
- rquickjs 0.12
- tokio 1.40
- taffy 0.11 (or yoga 0.5)
- serde, serde_json

### runts-ratatui
- runts-plugin
- runts-lib
- ratatui
- syn, quote (codegen)

## Known Limitations

1. **HIR Runtime**
   - useInput not supported
   - useEffect runs after first render
   - useStdin not available

2. **Layout Engines**
   - Yoga and Taffy have minor differences
   - Some edge cases differ between engines

3. **Text Rendering**
   - Unicode width calculation varies
   - ANSI color support differs

## Recommendations

1. **For 100% parity:** Accept static output in HIR for interactive apps
2. **For better coverage:** Add more edge case examples
3. **For reliability:** Add regression tests for known differences
4. **For performance:** Cache layout computations
