# Open parity findings and staged follow-up work

These are observations on frozen production source `faa120035`, not completed repairs or total parity. The parent packet maps executable and oracle identities.

* TextDecoder streaming differs in 116/232 chunk-boundary cases across eight encodings. Complete-buffer decoder checks do not qualify streaming.
* JSON module attribute error behavior differs in repeated/cached and isolated-URL matrices. Node cache reuse changes when host attribute validation occurs.
* Implicit derived constructors have length 1 instead of Node's 0, independent of parent arity.
* Color-convert exercises all 544 discovered conversion functions in 1,280 cases: 22 exact numeric differences. Math call traces identify same-input atan2/cbrt differences. No tolerance is used.
* A standalone Rust prototype (outside production) ports the pinned V8 cbrt/atan2 algorithms, using Rust libm's atan. It matches 36,065 exact-bit oracle cases at optimization level 1, including dense atan/atan2/cbrt inputs. The earlier libm atan2 probe and first prototype fail; their results remain retained. Integration, shared-mode regression checks and the package matrix are still required.
* Thirteen scratch package contracts have 378 matching assertions; the extended URL contract has 64 additional assertions. These are not in the frozen qualification harness. Draft fixture expectation mistakes are retained as failures, not runtime regressions or passes. The separate import/require driver now passes 898 assertions across 14 package receipts (the 13 new packages plus existing Jackspeak) on both engines; it remains outside the frozen qualification harness. The existing callable ledger projects 29 Jackspeak require paths from an import-only contract; it needs explicit branch receipts.
* No package performance phase has started. Stock Node --jitless does not expose WebAssembly on this host, including with --wasm-jitless. That limitation must not be counted as a Quench speed win.

Raw JSON results over 64 KiB are losslessly gzip-compressed. manifest.json records their uncompressed hashes. Guests retain the actual workspace paths used in these diagnostics.

The actual frozen Quench engine differs in 1,996 of the 36,065 math cases (986 cbrt, 900 atan2, 110 atan). Complete raw Node/Quench results and exact summary remain retained. Standalone Rust prototype success does not qualify an unbuilt production repair.
