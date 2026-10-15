# npm parity repairs, 2026-10-14

This milestone fixes shared runtime behavior exposed by pinned published packages.
Full 300-package/function qualification remains open on
[TOD-127](https://linear.app/todoxapp/issue/TOD-127).

The refreshed Node v26.11.1 baseline at integration 3063e667d has 248 API scenario
passes, 13 declaration-consumer passes, 38 Quench failures and one Node/platform
failure. The fixes produce 252 API scenario passes, 13 declaration passes,
34 Quench failures and the same musl/glibc oracle failure. Babel compilation
targets, ESLint, the TypeScript ESLint plugin and typescript-eslint now pass.

The shared changes are:

- Strict terminal calls retain the caller while a handler covers the call PC.
  Return-result optimization is distinct from eligibility to discard an activation.
  Both generic and known calls use the function's authoritative handler lookup;
  Wasm's explicit tail-call behavior is unchanged.
- Structured clone uses the ordinary own-property descriptor operation, including
  virtual dense/sparse array indices. It preserves elements, holes, getter ordering,
  cycles and aliases instead of skipping indices without explicit descriptors.
- Console captures canonical formatter functions at construction, so ESM contexts
  need no global require and public util export mutation cannot replace its formatter.

The reduced module-catch fixture now prints null like Node. Strict catch/finally,
array-clone, ESM console and affected console regressions pass against latest Node.
601 runtime tests and workspace/all-target checks pass. The instruction guard
passes all eight workloads (largest per-run increase +0.75%, Splay); 32 independent
completion checks retain exact output/status. This is a regression screen, with
no production performance claim. The final candidate includes timer-clock
integration 0234d909c, merged without conflicts in ded7d0b4c.

The new ESLint contract checks 30 public constructor/method/getter tags with
38 assertions on both engines, covering Linter and SourceCode. It leaves internal,
inherited token-store, returned AST/scope and other-export obligations explicit.
The coverage gate remains false; descriptor matches and passing smoke checks
never qualify all functions or all inputs. Complete final counts and per-path
obligations are in summary.json and the compressed reports.

Files prefixed pre-clock preserve the completed earlier candidate snapshot; the
final reports retain their exact later source, engine and fixture hashes. The full
upstream semver TAP suite still passes on Node and fails on Quench; the structured
observation records that failure rather than promoting the smaller semver corpus.

Ranking, installation, function-discovery limits and reproduction commands remain
in tests/npm-top300/README.md. Core cyclic-module and host capabilities (including
require(ESM), Worker, WebAssembly, N-API and missing module/util/OS methods) remain
open and are the next work. No package-name production bypasses are used.
