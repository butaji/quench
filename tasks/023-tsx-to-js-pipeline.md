# Task 023: Build TSX→JS Transpile Pipeline Using oxc_codegen or swc

**Priority:** P0-Critical  
**Phase:** 1 — rquickjs Dev Engine  
**Status:** ✅ COMPLETED
**ETA:** 4–6 hours  
**Depends on:** 022

## The Problem

rquickjs cannot execute raw TSX. We need a JS bundle that:
1. Has JSX desugared to `React.createElement`
2. Has TypeScript types erased
3. Resolves `import { Box, Text } from 'ink'` to bridge-injected globals

## Steps

### Step 1: Choose transpiler

**Option A: oxc_codegen** (already in deps as `oxc_codegen = "0.133"`)
- Parse TSX with oxc_parser (already wired)
- Use oxc_transformer to strip types + transform JSX
- Use oxc_codegen to emit JS

**Option B: swc** (would add dep)
- More mature transformer
- Heavier dependency

**Decision:** Try oxc first. If it cannot handle the JSX transform, add swc.

### Step 2: Implement `transpile_to_js(source: &str) -> String`

```rust
pub fn transpile_to_js(source: &str) -> Result<String> {
    let allocator = Allocator::default();
    let source_type = SourceType::default()
        .with_module(true)
        .with_typescript(true)
        .with_jsx(true);

    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.errors.is_empty() { bail!(...); }

    // Transform: TS → JS, JSX → React.createElement
    let mut program = ret.program;
    let transform_options = TransformOptions::default();
    Transformer::new(&allocator, ...).build(&mut program);

    // Codegen JS
    let codegen = Codegen::new();
    Ok(codegen.build(&program).source_text)
}
```

### Step 3: Resolve 'ink' imports to bridge globals

Replace:
```js
import { Box, Text, useInput } from 'ink';
```

With:
```js
const { Box, Text, useInput } = __runts_ink_bridge__;
```

Or inject globals directly before eval:
```js
var Box = __runts_ink_bridge__.Box;
var Text = __runts_ink_bridge__.Text;
// ... etc
```

### Step 4: Inject React/Preact shim

rquickjs needs a minimal `React` object:
```js
var React = {
    createElement: function(type, props, ...children) {
        return __runts_ink_bridge__.createElement(type, props, children);
    }
};
```

### Step 5: Test on ink-text-props

```rust
let js = transpile_to_js(&fs::read_to_string("examples/ink-text-props/tui/app.tsx")?)?;
let rt = QuickJsRuntime::new();
rt.register_ink_bridge()?;
let output = rt.eval(&js)?;
assert!(output.contains("HIGHLIGHTED"));
```

## Acceptance Criteria

- [x] `transpile_to_js` accepts TSX and emits runnable JS.
- [x] `import { Box, Text } from 'ink'` resolves to bridge globals.
- [x] JSX is desugared to `createElement` calls.
- [x] Type annotations are erased.
- [x] `examples/ink-text-props/tui/app.tsx` transpiles and evaluates without error.
