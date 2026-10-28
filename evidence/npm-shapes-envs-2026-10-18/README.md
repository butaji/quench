# Shapes, environments and activations behind the npm RSS gap (2026-10-18)

Host: `host.txt` (x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB RAM, no cgroup limits).
Comparators: Node v26.12.0 and Bun 1.4.2; Bun ran with this environment's
`BUN_OPTIONS=--smol` (see `../npm-gc-merge-2026-10-17/default-bun.txt`).
Base: v2-npm 8d0bcd033 (v2-merged 72681dacc merged in). Candidate: this commit (m13).

## Findings and fixes

1. **Every getter-only accessor owned two shapes.**
   - Accessor functions lived in the shape (`Vm::shape_accessors`), so a `Descriptor`
     transition that named a getter was never cached. Every object that defined one got
     its own chain: an `Add` plus a `Descriptor` shape, 96 B per property.
   - tsc's `__importStar`/`__createBinding`, esbuild's `__export` and Babel's re-export
     getters define these by the thousand. In typescript-eslint, each `import * as ts`
     copies about 2,700 getters, so 466k shapes (24 MiB) held 213k accessor properties.
   - Fix: a getter without a setter lives in the property's own slot (`SLOT_GETTER`). The
     shape names no function, so objects defining the same keys share one chain.
     `ShapeFacts::SLOT_GETTERS` marks such chains, so raw data reads (`own_property`,
     `symbol_property`) still see `undefined` there.
   - Also fixed: a data property made from an accessor without a `value` is now
     `undefined` (ValidateAndApplyPropertyDescriptor). It used to keep the data value the
     slot held before the accessor, as `Object.defineProperty(o, 'x', { writable: true })`
     showed after a getter replaced `x: 5`.
2. **Shape lookup indexes were quadratic on shared chains.**
   - Each object building a shared wide layout one key at a time asked "is this key
     absent?" on every intermediate shape. Each such walk counted toward that shape's
     lookup index, and each index holds the whole chain.
   - 80 objects of 2,700 data keys held 109 MiB of indexes (`variants.cjs data`). Sharing
     the getter chains (1.) pushed typescript-eslint from 263 to 367 MiB the same way.
   - Fix: a recorded `Add(key)` transition proves the key is absent from that shape. A key
     is only added to a shape that lacks it, and `transition_data_property_shape` now
     asserts that in debug builds. The two-property literal path also refuses
     `{ a, a }`, so the invariant no longer depends on its caller.
3. **Every environment allocated a scope box and two words per slot.**
   - Every activation environment boxed an `EnvironmentScope` of about 80 B to record
     only its program. It is now `EnvironmentScopeRef`: one word holding the program
     inline, or a box when the scope has eval, `with`, binding-site or dynamic-binding
     state.
   - Each slot was a 16-byte `EnvironmentSlot` enum. It is now one word: an owned value, or
     the owner of a shared slot encoded in the `TAG_DELETED` payload
     (`Value::shared_environment_slot`), which no guest operation produces.
   - typescript-eslint's 207k environments held 28.8 MiB of payload before this change.
4. **Every closure-creating activation recorded `this` and `new.target`.**
   - Any `MakeClosure` made a function's implicit bindings observable. Every call then
     pushed both into the frame's dynamic bindings, which boxed the environment scope as
     well (about 170 B per call of `function f(m, k) { return function () { ... } }`).
   - Only an arrow inherits them; any other closure has its own. The bindings are now kept
     when the function's own name or scope operations read them, or when it makes an arrow
     (`Function::activation_bindings_observable`).
   - The call plan's plain-activation rule is unchanged: it still excludes every
     closure-creating function.

## Probes (`probes.txt`, peak RSS MiB)

| probe | base | candidate | Node | Bun |
| --- | --- | --- | --- | --- |
| `importstar.cjs 80` (80 × `__importStar` of 2,700 keys) | 153.3 | 81.6 | 128.6 | 74.6 |
| `variants.cjs data` | 133.4 | 13.3 | 69.1 | 39.5 |
| `variants.cjs sharedget` | 45.3 | 13.6 | 74.0 | 49.4 |
| `variants.cjs function` | 121.7 | 74.0 | 116.5 | 66.9 |
| `variants.cjs closures` | 78.3 | 59.4 | 91.9 | 54.3 |
| `variants.cjs arrowclosures` | 66.1 | 47.3 | 97.6 | 52.6 |
| `closures.cjs plain` | 95.0 | 58.6 | 99.2 | 49.3 |
| `closures.cjs arrowWrap` | 75.2 | 55.4 | 100.0 | 50.9 |

## Effect on the npm top 300 (release builds; all 300 pass)

| run | lowest of 300 | median Quench / Node / Bun MiB |
| --- | --- | --- |
| m9 (base) | 272 | 20.4 / 63.5 / 36.9 |
| m13 (candidate) | 280 | 19.9 / 63.4 / 36.7 |

| package | base | candidate | Node | Bun |
| --- | --- | --- | --- | --- |
| typescript-eslint | 263.4 | 188.0 | 211.3 | 146.4 |
| @typescript-eslint/type-utils | 252.0 | 224.4 | 190.0 | 122.8 |
| playwright | 165.4 | 137.0 | 126.6 | 103.1 |
| jsdom | 115.1 | 110.5 | 171.3 | 96.8 |
| @testing-library/dom | 129.1 | 123.2 | 179.5 | 121.0 |
| vitest | 100.3 | 90.4 | 116.4 | 100.3 |
| @babel/traverse | 65.1 | 51.2 | 79.9 | 55.2 |
| @babel/types | 51.8 | 38.4 | 66.8 | 47.3 |

`npm-scenarios.json.gz` and `rss-report.txt` are m13. Every @babel package and vitest is
now lowest. The 20 not lowest:
- Eight @typescript-eslint packages: the TypeScript compiler's heap plus its compiled
  programs. About 90% of the 37k compiled functions are never called; the programs total
  about 65 MiB.
- playwright, jiti, jsdom, @testing-library/dom and undici.
- Seven declaration checks, 0.2–0.7 MiB above Bun. Their peak is TypeScript 7's native
  `tsc`, the same Go process for every engine. Quench also lacks `process.execve`, so the
  TypeScript 7 shim takes its `execFileSync` fallback there (fixed next).

## Instruction guard (`callgrind-guard.txt`)

`npm-icu-2026-10-16/npm-icu-fixed-callgrind-guard.sh` with iteration builds of the base
and the candidate. Per-run instructions are −0.02% to −0.51% on all eight fixtures.

An intermediate candidate measured +0.02% to +0.64%. Most of that was
`ThinBoxSlice::deref` recomputing the element offset's `Layout` on every access. That
offset is now the constant `ELEMENTS_OFFSET`.

## Tests

- quench-runtime lib: 703 passed. New tests:
  `getter_only_accessors_keep_their_getters_in_slots_and_share_shapes` and
  `only_arrow_closures_keep_their_creator_s_implicit_bindings`.
  `accessor_descriptor_transitions_do_not_root_values_in_the_cache` now covers both
  accessor representations.
- quench-node-test (release, oracle first on PATH): no new failures. Of the 18 failures:
  - five crypto tests fail because the oracle's own OpenSSL rejects `sha256-192`;
  - twelve fail identically on the base build;
  - `synchronous_child_timeout_closes_grandchild_pipes…` fails only under load.
