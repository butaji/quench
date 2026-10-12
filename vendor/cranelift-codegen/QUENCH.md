# Quench integration for Cranelift codegen

Upstream source: Cranelift `cranelift-codegen` 0.134.3, crates.io archive
SHA-256 `709f4b7c0fb57d952658d5b5c07fbdc4149acd7b7f0de9678ae754b9c981949a`,
upstream revision `5554cc1a651da536af2cc46c7324bdc085b162e3`, path
`cranelift/codegen`. The matching upstream Apache-2.0 WITH LLVM-exception
license is retained.

The local patch aligns the header of each small loop without calls to the
target's `small_loop_alignment` (x64: one 64-byte cache line; other targets
unchanged), so a hot inner loop's body sits in the fewest fetch windows
instead of wherever the preceding code leaves it. Upstream Cranelift aligns
only functions. `#![allow(warnings)]` keeps the workspace's `-D warnings`
from failing on upstream lints that a registry dependency's capped lints
would hide. `QUENCH.patch` records these changes against the published
0.134.3 crate source.
