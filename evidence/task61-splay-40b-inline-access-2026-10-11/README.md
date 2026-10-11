# Splay 40 B layout and inline-access gates (M4)

Platform: arm64 macOS, Mac16,10, 10 logical CPUs, 16 GiB RAM. Rust 1.99.0 / LLVM 23.1.2. Production profile uses the repository's 64-byte function alignment. Corpus revision: `64e1860c736c1b899708f4cd646721bc71e53d8b`.

## 40 B layout candidate

Source commit: `0bc3d8f790d81cefcad9fa03462d028dfa7adda4`. Baseline source trunk: aligned `32d8894d2`; baseline binary SHA-256 `43de900ad8553ef12db8e372a061492f160c1bff7a9af25b166b1ba7a6620509`. Candidate binary SHA-256 `8aa4e792e3ee228da9de17aa7614940b255bf474cc5f4f8282266c005c6aae1e`. Runner SHA-256 `f8e02485d1724f130fb318fdf26089896dcdc540c03131173464d92301765bfa`.

The schema-1 resumable guard required 7 clean pairs; all fixtures reached at least 9 clean pairs and completed. It **rejected** the layout. DeltaBlue regressed +0.898% cycles (10 clean pairs; 95% paired interval +0.422% to +1.336%) and +0.954% instructions. RayTrace regressed +0.870% cycles (11/11; interval +0.397% to +1.502%) and +0.589% instructions. Other fixtures had no detected cycle regression. Splay improved −1.496% cycles (10 clean pairs; interval −2.366% to −0.257%), instructions −0.137%, and max RSS −20.56% (122,847,232 B to 97,615,872 B). RSS stayed within the 0.5% budget on all eight fixtures. The all-eight failure keeps this layout off trunk.

For the fixed-work proxy, candidate Splay marginal cycles were 5.364M/run and max RSS 97.62 MB. Against the prior M4 references (Node `--jitless` 4.679M marginal cycles; Bun no-JIT 58.70 MB), this is approximately 1.15x speed and 1.66x RSS. These are diagnostics, not stock-harness qualification.

## Arena-locator source audit and direct inline-access experiment

`Object.properties` remains a fixed `ValueVec` field; its `start` encodes the `ValueArena` locator. Arena-backed property reads first see `has_inline_property_storage() == false` and take `ValueArena`, without reading the `ObjectStorage` union arm. Thus the suggested arena-locator movement does not exist in this candidate source.

A separate change made `inline_properties()` select directly between the ordinary inline union arm and the extras arm, relying on the inline sentinel to exclude arrays. Source commit: `3d31b22b4`. The runtime suite passed (548/548 plus 6 subset tests). Against the prior layout candidate, the targeted 11/11-clean Splay pair got +1.35% marginal cycles, +0.043% instructions, and flat RSS; RayTrace got +0.184% cycles, +0.027% instructions, and +0.158% RSS. This inline-access change is rejected as no improvement. Raw pair reports are included.

`cargo test --profile bench-throughput -p quench-runtime --features profile-memory` passed (548 runtime tests plus 6 subset tests), and `cargo test --profile bench-throughput -p quench-wasm` passed (31 tests); `git diff --check` passed. The external WAST corpus is absent from this checkout, so no full WAST-suite claim is made.
