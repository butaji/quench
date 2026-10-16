# npm filesystem/provenance integration qualification

Total Node/package/function parity is **not reached**. No package speed win is claimed; the requested performance phase after parity has not started.

Production source **b8b02f99245ee4e3392c84b75480733d7f135802** merges integration **f0dc38338** with the previously published npm repairs. Frozen engine SHA256 **8bf0b426e7f8cf23df979d055b84d91c21e6ca5878f88590618aa1773a07c7fe**. It passes **632 fast and 632 optimized Core tests**, **296 selected Node API fixtures**, **four host lifecycle tests**, and workspace/all-target checks. The build script records the explicitly resolved official Node 26.12.0 path/version/SHA256; the oracle hash is **caa24debe02770c92a70677e2a62ca98890150e7ac1c17bcfe31bcc98cc89943**.

All **300** scenario outcomes: **273 API passes, 13 declaration passes, 13 Quench failures and one musl-on-glibc oracle failure**. The previous published engine has identical outcomes; no gains or regressions from this integration sync. Surface: **218 unqualified matches / 73 differences / nine oracle failures**. Callable coverage remains **464/70,236** projected paths, qualified=false; the Jackspeak branch audit in the previous packet remains open for the production harness.

Complete-buffer decoder corpora **896 / 19,968 / 1,310,720** cases and **896 WPT URL cases** match Node on this exact new engine. All-eight matched instruction guard and **32 independent exact completions** pass. Largest per-run increase is **+0.019142% (earley-boyer)**, with no >1% finding. Startup counts remain separate. This compares Quench cuts and is not a Node/package performance measurement.

Baseline production source **faa120035**, published on v2-npm at **1f36ad64d**, frozen executable SHA256 **d6482c1dbf34b2d0045f033404c6e469c916e893b7ceb8c205a610a7ce0681ff**. The baseline package report is a **reused retained observation**, not a fresh execution. Its exact report hash, executable hash, inventory, lock, scenario and Node oracle hashes are checked against the new run; npm-fs-sync-baseline-scenarios-reuse.json records this lineage. The instruction baseline is freshly measured.

The CommonJS merge preserves npm's public cache/synchronous ESM behavior, owner provenance checks and private crypto symbol authority. It removes the superseded filename-suffix/source-rewriting crypto adapter. No legacy module_cache field references remain. Source changes and all measurements stay distinct from the staged math/constructor/corpus repairs under the previous packet's followup/.

Replay orchestration: npm-fs-sync-build-checks.sh and npm-fs-sync-suites.py. Decoder/WPT guests and licensed data are in ../npm-esm-codecs-2026-10-15/. Large raw results are losslessly gzip-compressed; raw Cargo log EOF whitespace is retained. summary.json fields are in npm-fs-sync-summary.json.
