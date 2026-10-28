# Top-300 npm: math parity and explicit entry contracts

Production source: `cac5edc64d41ce6e76b0bd800e193fbb14493f4b`, including v2-merged `bc9e0080c9f4b7f1c5bf2eb867b1af938f9bacee`.
Frozen candidate SHA-256: `3c83e9e92ce727b70f931eddf6f124861f80dda08c824ef968a3b7036e1c6c53`.
Oracle: official Node v26.12.0, SHA-256 `caa24debe02770c92a70677e2a62ca98890150e7ac1c17bcfe31bcc98cc89943`.
Fresh baseline: source `b8b02f99245ee4e3392c84b75480733d7f135802`, published through `1ba3a21ed7eafcf9b1b7583444195710734b445f`. Both engines used this source's expanded harness. No baseline observation was reused.

The Rust runtime implements V8/fdlibm atan, atan2 and cbrt results, preserving coercion order and signed zero. Implicit derived constructors forward arguments with length zero. Package contracts now exercise import and require separately for 16 packages; color-convert includes every 544 rounded/raw conversion function, with 2,176 exact numeric vectors and null/undefined/metadata checks. Symbol contracts require verified well-known-symbol identity; same-description symbols remain separate obligations.

Validation: workspace/all-target check; 636 fast and 636 optimized core tests; 300 selected Node API tests against the exact Node oracle; four Node lifecycle tests; three coverage unit tests. The actual frozen CLI matches all 36,065 exact math cases, the constructor oracle, and 7,814 assertions across the 16 package corpora. Existing decoder corpora match on 896 mixed cases, 19,968 single-byte values and 1,310,720 two-byte cases; all 896 URL WPT cases match.

Fresh candidate scenarios: 273 behavior passes, 13 declaration passes, 13 Quench failures and one platform oracle failure. Fresh baseline: 272 / 13 / 14 / one. Color-convert improves and no scenario regresses. The surface scan remains unqualified: 221 matching projections, 70 differences and nine oracle failures. The coverage ledger records 1,664 checked projections out of 70,236 discovered callable paths; these are finite contract projections, not proof for every function or arbitrary inputs. Legacy alias normalization remains explicitly limited. Total Node parity is false.

All eight instruction guards ran, with all 32 exact completion receipts verified. Largest increase: raytrace +0.615121%; no increase exceeds 1%. This compares Quench builds, not Quench package performance against Node. Package performance and Node --jitless wins remain unmeasured and open.

Raw logs, corpus outputs, source/engine hashes, runner scripts, reports and instruction counts are retained. Files over 64 KiB are gzip-compressed without altering their bytes. The summary is generated from completed receipts. A separate decoder repair was being drafted after this source was frozen; it is excluded from this qualification and this packet's production source.
