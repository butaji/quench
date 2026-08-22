# Stage 09 — Full acceptance

## Contract
Treat `tests/node` as the primary oracle: `test/parallel`, `test/es-module`, and applicable `test/common`/`test/fixtures`; supplement standards globals with WPT and language changes with Test262. Native addons, Node-API, internet-only tests, and platform-impossible behavior remain explicitly classified rather than silently skipped.

## Required gates
1. Reconcile the normalized API declaration/IR with generated registry, wrappers, builtin inventories, dispatch, and ordinary tests.
2. Run `cargo run -p quench-node-test --bin run-compat -- --quiet` and `cargo run -p quench-node-test --bin run-parallel`.
3. Run staged coverage: `cargo run -p quench-node-test --bin run-stages -- --from 1 --to 8 --continue` when available, plus `tools/compat-coverage.sh`, `tools/check-application-stages.sh`, Prettier, and `git diff --check`.
4. Exercise the CLI and representative npm consumers (Hono, Express, Fastify, Koa, ws, GraphQL, zod, lodash, dayjs, pino) from published packages through `quench-node`.
5. Record pass/fail/skip/platform-limited evidence in runner output and update `docs/NODE-COMPAT.md`; do not claim full compatibility from export presence.

## Exit criteria
No unclassified failures or skips in the declared target, all affected callsites and generated consequences are synchronized, and a fresh full-suite run passes on the supported host platforms.
