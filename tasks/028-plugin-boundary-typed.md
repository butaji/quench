# Task 028: Replace Plugin JSON String Boundary with Typed HIR Transfer

**Priority:** P1-High  
**Phase:** 3 — Compile Path  
**ETA:** 4–6 hours  
**Depends on:** 020

## The Problem

The plugin boundary currently does this:

```rust
// 1. Parse TS to typed HIR
let hir_module: hir::Module = parse_source(source, is_tsx)?;

// 2. Serialize to JSON
let hir_json = serde_json::to_string(&hir_module)?;

// 3. Inject extra fields into JSON string (!)
let mut hir_value: serde_json::Value = serde_json::from_str(&hir_json)?;
hir_value["source_path"] = json!(rel_path_str);
let hir_with_plugin_data = serde_json::to_string(&hir_value)?;

// 4. Pass JSON string to plugin
let rust_code = plugin.codegen_module(&hir_with_plugin_data)?;

// 5. Plugin deserializes back to its own Module type
//    (which may or may not match the core HIR shape)
```

This is:
- **Lossy**: Typed enums become untyped JSON objects.
- **Fragile**: Changing a field name in `hir::Module` silently breaks every plugin.
- **Slow**: 2 serialize + 2 deserialize operations per file.
- **Unmaintainable**: Plugins cannot use Rust pattern matching on HIR nodes.

## Why This Matters

- EXECUTE.md requires `runts compile` to produce binaries that match deno/HIR output.
- If the plugin receives garbage HIR because of a JSON mismatch, the generated Rust is wrong.
- `runts-ratatui` and `runts-fresh` plugins are large (500–1,300 lines). They need reliable data.

## Steps

### Step 1: Add `hir::Module` to the plugin trait

Change:

```rust
pub trait Plugin {
    fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError>;
}
```

To:

```rust
pub trait Plugin {
    fn codegen_module(&self, module: &runts_plugin::hir::Module) -> Result<String, PluginError>;
}
```

Wait — the plugin crate defines its own `hir::Module` which is metadata-only (`source_path`, `route_info`, `items_json`). It does NOT contain the AST.

The real issue is that `runts_plugin::hir::Module` is a **different type** from `crate::transpile::hir::Module`.

### Step 2: Unify the types

**Option A**: Re-export core HIR in `runts_plugin`.

In `crates/runts-plugin/src/lib.rs`:

```rust
pub use runts_hir::Module as HirModule;
```

But `runts_plugin` cannot depend on the main `runts` crate (circular dependency).

**Option B**: Move `hir::Module` and all HIR types into `crates/runts-hir` (a new crate).

Both `runts` (core) and `runts_plugin` depend on `runts_hir`.

This is the correct architecture:

```
crates/
├── runts-hir/       # NEW: all HIR types, no logic
│   ├── src/lib.rs   # Module, Expr, Stmt, JSXExpr, etc.
│   └── Cargo.toml
├── runts-plugin/    # depends on runts-hir
│   └── src/lib.rs   # trait Plugin { fn codegen_module(&self, module: &runts_hir::Module) -> ... }
├── runts/           # depends on runts-hir, runts-plugin
│   └── src/...
├── runts-ratatui/   # depends on runts-hir, runts-plugin
│   └── src/...
└── runts-fresh/     # depends on runts-hir, runts-plugin
    └── src/...
```

### Step 3: Create `crates/runts-hir`

Move these from `src/transpile/hir/` into `crates/runts-hir/src/`:

- `base.rs` → `lib.rs` (or keep as `base.rs` + `lib.rs` re-exports)
- `expr.rs`
- `stmt.rs`
- `pat.rs`
- `effects.rs`
- `ownership.rs`
- `type_gen.rs`
- `type_to_rust.rs`

Remove `quote_codegen.rs` from `runts-hir` — it stays in core (it depends on `quote` crate).

### Step 4: Update all `use` statements

In `runts` core:

```rust
// OLD
use crate::transpile::hir::Module;

// NEW
use runts_hir::Module;
```

In `runts-plugin`:

```rust
pub trait Plugin {
    fn codegen_module(&self, module: &runts_hir::Module) -> Result<String, PluginError>;
}
```

In `runts-ratatui` and `runts-fresh`:

```rust
use runts_hir::Module;
use runts_plugin::Plugin;
```

### Step 5: Delete JSON round-trip

In `src/commands/build/mod.rs`, replace:

```rust
let hir_json = serde_json::to_string(&hir_module)?;
let mut hir_value: serde_json::Value = serde_json::from_str(&hir_json)?;
// ... mutate JSON ...
let hir_with_plugin_data = serde_json::to_string(&hir_value)?;
let rust_code = plugin.codegen_module(&hir_with_plugin_data)?;
```

With:

```rust
let rust_code = plugin.codegen_module(&hir_module)?;
```

If plugins need `source_path`, add it to `hir::Module` as a field:

```rust
pub struct Module {
    pub source: String,
    pub source_path: Option<String>,  // NEW
    pub items: Vec<ModuleItem>,
    pub types: HashMap<String, TypeDef>,
}
```

### Step 6: Fix plugin implementations

`runts-ratatui/src/plugin.rs` and `runts-fresh/src/plugin.rs` currently parse JSON:

```rust
fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError> {
    let module: serde_json::Value = serde_json::from_str(hir_str)?;
    // ... traverse Value ...
}
```

Change to:

```rust
fn codegen_module(&self, module: &runts_hir::Module) -> Result<String, PluginError> {
    for item in &module.items {
        match item {
            runts_hir::ModuleItem::Decl(runts_hir::Decl::Function(func)) => {
                // real pattern matching
            }
            // ...
        }
    }
}
```

This is a large refactor. Do it incrementally:
1. Create `crates/runts-hir` and move types.
2. Update `runts-plugin` trait.
3. Update `runts-ratatui` to use typed HIR (keep JSON fallback temporarily).
4. Update `runts-fresh`.
5. Remove JSON path from core build.

## Acceptance Criteria

- [ ] `crates/runts-hir` exists and compiles independently.
- [ ] `runts-plugin::Plugin::codegen_module` takes `&runts_hir::Module`.
- [ ] No `serde_json::to_string` / `from_str` round-trip in the build pipeline.
- [ ] `runts-ratatui` and `runts-fresh` compile with the new trait.
- [ ] `cargo build` passes for the entire workspace.

## Notes

- This is a **structural refactor**, not a logic change. Every generated line of Rust should be identical before/after.
- If moving HIR types breaks the parser (which is in core), keep the parser in core and only move the type definitions to `runts-hir`.
- Consider using `cargo check -p runts-hir` frequently during the move.
