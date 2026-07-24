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

`urlencoding` only handles `%`-encoding/decoding — keep it for
`encodeURI`/`decodeURI`. ES modules (stage 53) additionally need full
URL Standard resolution. **Decision:** add `url` (rust-url) +
`data-url` (data: payload decoding) when stage 53 starts; do not
replace `urlencoding`, which is correct for the URI builtins.

---

## Accepted — landing at their stage (decisions made 2026-07-24)

Each row lands with a failing test and a `Cargo.toml` diff in the same PR.

| Crate | Stage | Decision |
|---|---|---|
| `oxc_semantic = "0.47"` | R17 | **Adopt.** Early errors on the oxc AST pre-lower; the `oxc` umbrella crate does not include it (Cargo.lock-verified). Deletes thousands of hand-rolled LOC in `lower/`. |
| `bumpalo = "3"` | R19 | **Adopt** (not `bump_scope` — unproven). Arena for eval frames → parser → Value constructors. |
| `string_interner = "0.20"` | R21 | **Adopt** and delete the unused hand-rolled `src/interner.rs` in the same diff. Hashing via the vendored `rustc-hash`; `fnv` is rejected (stale, slower). |
| `url` + `data-url` | 53 `modules` | **Adopt** for URL Standard resolution + `data:` payload decoding. `urlencoding` stays for the URI builtins. |
| `regex` + `unicode-perl` | 84 `RegExp` | **Adopt as secondary engine only.** Dispatched when a pattern has `\p{}`/`\P{}` and no lookaround/backrefs; `regress` stays primary (only it has backrefs/lookbehind). Residual gap: patterns needing both. |
| `temporal_rs` + `zoneinfo_rs` | 120 `Temporal` | **Adopt.** Powers Boa (94.12%), Kiesel; V8's Temporal is being implemented on it. Pin the version whose spec snapshot matches the test262 checkout when stage 120 starts. |

---

## Rejected / not fit

| Crate | Stage | Why rejected |
|---|---|---|
| `swc_ecma_compat_es2017` | async→generator (38, 40, 97–99) | Operates on the swc AST — requires a second parser stack (`swc_ecma_*`, ~10+ crates) + codegen + oxc re-parse. Rejected 2026-07-24: single parser (OXC) only. Async is hand-rolled in `eval/` (S4; Boa reaches 94.12% the same way). |
| `tokio` | Promise / async | Overkill. Full multi-threaded runtime; a hand-rolled job queue is purpose-built for a microtask queue. |
| `smol` | 113 `Promise` | Same role, smaller — still unnecessary. Boa runs a hand-rolled executor; the existing `builtins/promise/` job queue stays. |
| `async-executor` | Promise / async | Same role as smol but more bytes. |
| `unicode-segmentation` | 82 `String` | Spec iterates **code points**, not grapheme clusters, for `String.prototype[Symbol.iterator]`. No current stage needs graphemes. |
| `bytemuck` | 102 `TypedArray` | `f32::from_bits`-style conversions suffice; adopt only if profiling shows bulk transmutation hot. |
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
| Temporal | 9 | Stage 4 (2026-03) | `temporal_rs` + ICU4X | None. `temporal_rs` also underpins V8's in-progress Temporal implementation. |
| Modules | 5 | ES2020+ | `url` (URL Standard) | `data:` URLs — `data-url` or inline parsing needed |
| Date | 3 | ES2023 | `chrono` (partial) | R3: `builtins/date.rs` hand-rolls leap-year math without importing chrono. Fix: use `chrono::NaiveDate` + `chrono::Utc` in `builtins/core/date.rs`. |
| async/await | 7 | ES2017 | hand-rolled in `eval/` (Boa-style, ~500 LOC) | oxc_transformer has no async-to-generator (confirmed); swc rejected (second parser — OXC only). Boa proof: hand-rolled works. |

Last verified: 2026-07-24.
