# LOC reduction opportunities

Analysis of the current codebase identifying structural duplication,
boilerplate, and dispatch-table sprawl that can be eliminated.

## Current state

| Metric | Count |
|---|---|
| Total Rust LOC | ~56,500 |
| Source files | 286 |
| `include!` directives | 134 |
| Files existing only for `include!` | 134 |
| `Builtin` enum variants | 579 |
| Property-dispatch match arms | 468+ |
| Builtin-handler dispatch arms | 102+ |
| `builtin_meta` submodules | 21 |
| Intl modules with identical structure | 8 |

## Ranked opportunities

### 1. Unified builtin definitions (saves ~3,500 LOC)

Every builtin's metadata is handwritten in 4-6 separate locations across
25+ files. A single declarative definition per builtin would generate the
`Builtin` enum, property tables, constructor metadata, and dispatch layer.

**Files affected**:
- `ops.rs` (586 lines, 579 enum variants)
- `builtins/props.rs` + subfiles (972 lines, 281 match arms)
- `builtin_meta/mod.rs` + 21 submodules (2,313 lines, 143 match arms)
- `intl/mod.rs` (724 lines, property dispatch)
- `vm/vm_builtins.rs` (882 lines, 102 match arms)
- `vm/vm_dispatch.rs` (470 lines, 94 match arms)

**Target**: ~150 declarative definitions at ~15 lines each (2,250 LOC)
replacing ~5,900 LOC of hand-maintained tables.

### 2. Compact property descriptor type (saves ~700 LOC)

Properties use `Vec<(String, Value)>` with ghost descriptor keys
(`\0quench:descriptor:\0name`). A `PropertyDescriptor` struct with
`{ value, writable, enumerable, configurable }` fields would eliminate
the sentinel-key scheme and collapse 7-line descriptor blocks to 1 line.

**Files affected**: `builtins.rs` (27 occurrences), `construct.rs` (14),
`builtins/object.rs` (12), `property_define.rs` (5), plus ~10 more files.

### 3. Intl constructor trait/macro (saves ~640 LOC)

8 intl modules each implement the same pattern: `construct()`, options struct,
`from_options()`, `build_object()`, `prototype_method()`, `resolved_options()`.
A `define_intl_constructor!` macro or `trait IntlConstructor` would collapse
~100 lines of boilerplate per module to ~20 lines of config.

**Files**: `intl/number.rs` (896), `intl/relative.rs` (622),
`intl/datetime.rs` (512), `intl/collator.rs` (177), `intl/list.rs` (220),
`intl/plural.rs` (195), `intl/displaynames.rs` (207),
`intl/segmenter.rs` (339).

### 4. Remove double-dispatch (saves ~350 LOC)

Property lookup is two-step: `(Builtin, key) → Builtin` then
`Builtin → handler fn`. A single-step `HashMap<&str, HandlerFn>` or
`phf_map` would eliminate the intermediate `Builtin` indirection.

**Files**: `vm/vm_builtins.rs` (early_dispatch + is_simple_builtin +
execute_simple_builtin), `builtins/props.rs`.

### 5. String-literal `.to_string()` verbosity (saves ~250 LOC)

~450 calls to `.to_string()` on static string literals across the intl
modules and builtins. A `static_str` helper or `.into()` usage would
cut 7-10 characters per occurrence.

**Files**: `intl/number.rs` (81), `intl/relative.rs` (50),
`builtins.rs` (48), `intl/locale.rs` (47), `intl/number_format.rs` (37),
plus 8 more files.

### 6. Generic Map/Set collection (saves ~250 LOC)

`map.rs` (484) and `set.rs` (334) share identical insertion, deletion,
lookup, and iteration logic. `Set` is `Map` with `V = ()`. The
`canonicalize_key`/`canonicalize_value` and `same_value_zero` wrappers
are identical.

### 7. Internal slots as struct fields (saves ~150 LOC)

Sentinel-key prefixes (`\0prototype`, `\0error_slot`,
`\0quench:descriptor:\0`, `\0quench:deleted:\0`) are a workaround for
flat property storage. Dedicated fields on `ObjectData`/`MapData`/`SetData`
would eliminate sentinel-key comparison logic.

### 8. `include!` → proper modules (structural)

134 `include!` directives prevent proper encapsulation, confuse tooling,
and allow files to grow without clear API boundaries. Converting to `mod`
declarations forces clean interfaces and reveals natural deduplication
points. This is a prerequisite for several other reductions.

## Summary

| # | Change | ~LOC Saved | Risk |
|---|---|---|---|
| 1 | Unified builtin definitions | 3,500 | High |
| 2 | Compact descriptor type | 700 | Medium |
| 3 | Intl constructor trait/macro | 640 | Low |
| 4 | Single-step dispatch | 350 | Medium |
| 5 | `.to_string()` cleanup | 250 | Low |
| 6 | Generic Map/Set collection | 250 | Low |
| 7 | Internal slots as fields | 150 | Medium |
| 8 | `include!` → proper modules | 0 (structural) | Medium |
| **Total** | | **~5,840** | |

## Relation to architecture.md

These reductions align with the existing architecture doctrine:

- **"One declaration generates every mechanical consequence"** (Frozen Doctrine #7):
  items 1, 2, 3, 4, 6 are all instances of the same fact being handwritten
  in multiple places.
- **"Generated LOC, binary text, static data, caches, and native code all
  count toward the memory and complexity budget"** (Frozen Doctrine #18):
  the 5,900 lines of dispatch tables are generated-manually, not
  generated-mechanically.
- **"Never represent the same semantic fact twice"** (Frozen Doctrine #1):
  the double-dispatch (item 4), Map/Set duplication (item 6), and intl
  boilerplate (item 3) all violate this.
- **"Use declarative macros as the source of truth for mechanical runtime
  data"** (docs/architecture.md): items 1, 2, 3, 4 are all candidates for
  the `builtin!` / `value!` declarative generation approach already planned.