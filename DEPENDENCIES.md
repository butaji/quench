# Dependencies

Policy: every crate earns its place by replacing more LOC than it adds, or
by covering spec semantics no hand-rolled code can match. A hand-rolled copy
—including a thinly-disguised `chrono_*` helper that never imports the crate—is
forbidden. A new crate needs a row here in the same diff.

Confirmed crates: `oxc`, `regress`, `chrono`, `num-bigint`, `serde_json`,
`urlencoding`, `indexmap`, `rustc-hash`, `phf`, `tracing`, `anyhow`,
`walkdir`, `tempfile`, `serial_test`.

---

## Confirmed — in use

| Crate | Version | Purpose | File |
|---|---|---|---|
| `oxc` | 0.47 | Parser (oxc → internal AST) | `Cargo.toml` |
| `regress` | 0.11 | RegExp exec (ES2018 syntax targeting) | `Cargo.toml` |
| `chrono` | 0.4 | Date math, timestamp conversion | `Cargo.toml` |
| `num-bigint` | 0.4 | BigInt arithmetic | `Cargo.toml` |
| `serde_json` | 1 | JSON parse/stringify | `Cargo.toml` |
| `urlencoding` | 2 | URI encoding/decoding (see note) | `Cargo.toml` |
| `indexmap` | 2 | Ordered property storage (`IndexMap<Key, Prop>`) | `Cargo.toml` |
| `rustc-hash` | 2 | Fast `FxHashMap` for internal slots | `Cargo.toml` |
| `phf` | 0.14 | Compile-time static maps | `Cargo.toml` |
| `tracing` | 0.1 | Logging/tracing | `Cargo.toml` |
| `anyhow` | 1 | Error propagation in test harness | `Cargo.toml` |
| `walkdir` | 2 | Recursive directory traversal | `Cargo.toml` |
| `tempfile` | 3 | Temp files in tests | `Cargo.toml` |
| `serial_test` | 3 | Serialized test execution | `Cargo.toml` |

### Note on `urlencoding`

`urlencoding` only handles `%`-encoding/decoding. The ECMAScript `import`
spec requires full URL resolution (scheme parsing, path normalization,
`data:` URLs, bare specifiers). Consider upgrading to `url` (rust-url) which
implements the URL Standard and covers `data:` URLs natively.

---

## Candidate — not yet added

Each row needs a failing test, a `DEPENDENCIES.md` diff, and a verification
run before landing.

### High confidence (research-verified, spec-aligned)

| Crate | Stage | Difficulty | Why | Risk |
|---|---|---|---|---|
| `url` | 53 `modules` | 5 | Supersedes `urlencoding` for URL Standard compliance. Covers `import "https://..."`, bare specifier resolution, and `data:` URLs. | Low. Drop-in upgrade. |
| `oxc_semantic` | R17 | 4 | Language early errors (duplicate `let`, TDZ violations, redeclaration) via `oxc_semantic::SemanticAnalysis`. Replaces thousands of LOC of hand-rolled checks in `lower/`. | Low. Already using oxc. Add `oxc_semantic` feature if available, or use existing `oxc` re-exports. Verify `ctx.semantic()` hook in `parser.rs`. |
| `temporal_rs` | 120 `Temporal` | 9 | Stage 4 ECMAScript spec. Powers Boa (94.12% test262), Kiesel, and **V8/Chrome 144**. 8 types via ICU4X + Diplomat. | Low-medium. Evaluate API surface vs. ES spec version. Spec was stable as of 2025-09. Evaluate before committing. |
| `regex` + `unicode-perl` | 84 `RegExp` | 7 | `regress` (ES2018) does NOT support Unicode property escapes `\p{}`. `regex` with `unicode-perl` covers `\p{Script}`, `\p{Emoji}`, etc. | Low. Evaluate replacing `regress` if `regex` covers ES2018 backreferences + lookbehind too. |

### Medium confidence (research-verified, needs validation)

| Crate | Stage | Difficulty | Why | Risk |
|---|---|---|---|---|
| `swc_ecma_compat_es2017::async_to_generator` | 40, 38, 113, 97-99 | 7 | Unlocks ~2,500 async tests. `docs.rs/swc_ecma_compat_es2017` exposes the transform. | **High.** Full `swc_ecma_*` stack (~10+ crates) alongside existing `oxc` (0.47). Validate: (a) works standalone without swc parser, (b) no version conflict. Fallback: hand-roll ~500 LOC in `lower/`. Boa proof-of-concept confirms hand-rolled works. |
| `smol` | 113 `Promise` | 5 | Single-threaded async executor for microtask queue. Boa uses its own executor (not smol); the hand-rolled job queue in `builtins/promise/` works today. | Medium. Evaluate against integration complexity. |
| `data-url` | 53 `modules` | 4 | Parses `data:` URLs per Fetch Standard. `import "data:text/javascript,..."` needs this. | Low. Small, focused crate. |

### Low priority / evaluate later

| Crate | Stage | Difficulty | Why | Risk |
|---|---|---|---|---|
| `unicode-segmentation` | 82 `String` | 3 | Grapheme cluster iteration. The spec uses code point iteration for `String.prototype[Symbol.iterator]`, not grapheme clusters. Not needed unless `String.prototype` methods specifically require it. | Low priority. |
| `bytemuck` | 102 `TypedArray` | 2 | Safe typed array element transmutation. `f32::from_bits` works for single values. Bulk operations would need it, but that's not the bottleneck. | Not needed now. Re-evaluate when TypedArray bulk operations are profiled. |

---

## Rejected / not fit

| Crate | Stage | Why rejected |
|---|---|---|
| `tokio` | Promise / async | Overkill. Full multi-threaded runtime; smol or hand-rolled is purpose-built for a microtask queue. |
| `async-executor` | Promise / async | Same role as smol but more bytes. |
| `fancy-regex` | RegExp | Supports lookbehind but not ES2024 `unicode_sets` mode for `\p{}`. |
| `re2` | RegExp | No backreferences, lookahead, or Unicode property escapes — too limited for ES spec. |
| `time` crate | Date / Temporal | Covers basic Date math but not the full Temporal API. `temporal_rs` + `chrono` covers this. |
| `wasmtime` | 118 `ShadowRealm` | ShadowRealm is a JS-level isolated global per spec, not a WASM sandbox. |

---

## macOS / Darwin notes

No Darwin-specific code needed for any remaining test262 stage:

- **Atomics** (stage 106): `std::sync::atomic` works natively on macOS.
  test262 also tests atomics on macOS; no platform branching needed.
- **Date / Temporal**: `chrono` and `temporal_rs` handle timezone math
  portably in userspace — no Darwin `CFDate` / `NSTimeZone` APIs.
- **SharedArrayBuffer** (stage 101): requires cross-origin isolation
  headers (`Cross-Origin-Embedder-Policy`); the test262 harness skips
  these tests when headers are absent — no OS-level work required.
- All file I/O is in the test harness (`tools/run-each.sh`), not the
  runtime engine.

No `cfg(target_os = "macos")` branches should appear in `src/`.

## Spec / crate alignment

| Stage | Difficulty | Spec version | Crate coverage | Gap |
|---|---|---|---|---|
| RegExp | 7 | ES2024 | `regress` (ES2018) + `regex` (Unicode) | Unicode property escapes `\p{}` — `regex` fills gap |
| Temporal | 9 | Stage 4 (2026-03) | `temporal_rs` + ICU4X | None. `temporal_rs` is the reference impl for V8/Chrome. |
| Modules | 5 | ES2020+ | `url` (URL Standard) | `data:` URLs — `data-url` or inline parsing needed |
| Date | 3 | ES2023 | `chrono` (partial) | R3: `builtins/date.rs` hand-rolls leap-year math without importing chrono. Fix: use `chrono::NaiveDate` + `chrono::Utc` in `builtins/core/date.rs`. |
| async/await | 7 | ES2017 | `swc_ecma_compat_es2017::async_to_generator` or hand-rolled | **Verify before committing swc.** oxc_transformer does NOT have async-to-generator (confirmed). Boa proof-of-concept: hand-rolled works. |

Last verified: 2026-07-23.
