# Web framework benchmarks (no JIT)

The measurement-only `web` Rust binary runs the multi-workload
[SaltyAom suite](https://github.com/SaltyAom/bun-http-framework-benchmark)
at commit `f654bcd175165bce2e3c83b65eaecd8e34294ffd`. Its original framework
sources, background routes, JSON payload, video and binary dependency lock stay
unchanged. Quench runs the **same Node-target bundle** as Node, through
`quench-node`; it does not emulate the Bun host APIs. Bun-native Elysia is a
separate framework/runtime combination, not an isolated runtime comparison.

## Correctness prerequisite

The user requires **full parity with the latest stable Node before optimization**.
As checked against nodejs.org/dist/index.json on 2026-10-15, that oracle is
Node **v26.12.0**, released 2026-10-14 (Current stable, not just LTS).
Refresh the release index before subsequent optimization work; verify the official
binary and test-source archives against that release's SHASUMS256.txt.

Run verification before load testing:

```sh
cargo run --release -p quench-bench --bin web -- \
  --suite work/web-upstream --out evidence/web-verification-NEW \
  --quench target/production/quench-node --node /path/to/latest-stable-node \
  --bun bun --bombardier bombardier --rounds 1 --verify-only
```

`--verify-only` performs no load tests. It retains independent correctness rows
so one failed workload cannot hide another. This web check is only one part of
Node parity: the upstream Node inventory and npm function-level coverage must
also be qualified, including errors, descriptors, identity, ordering, process
exit and host effects. Skips, unsupported cases and untested behavior are gaps;
a passing framework subset must never be called full parity. Optimize only once
that broader requirement is met. Current counterexamples and owner coordination:
[TOD-134](https://linear.app/todoxapp/issue/TOD-134),
[Node lane](https://linear.app/todoxapp/issue/TOD-20), and
[npm qualification](https://linear.app/todoxapp/issue/TOD-127).

## Node library implementation

The `tests/node` submodule is pinned to v26.12.0. The Rust build embeds its
unmodified JavaScript library and bundled Undici as guest input to Quench’s
interpreter; it does not invoke another JavaScript runtime. Initialize the
submodule before building (`git submodule update --init tests/node`). Its
original license and third-party notices are retained in `tests/node/LICENSE`.
Native HTTP/2 transport and many other Node APIs remain unsupported; using
Node’s library algorithms does not establish full API parity.

## Setup

From the repository root, install Bun, curl, GNU `timeout`, `sha256sum`, and
[bombardier](https://github.com/codesenberg/bombardier) v2.0.2. The orchestrator
uses GNU command-line tools on Linux; macOS needs compatible tools on PATH.
Bun is needed as an offline build tool even when only Node/Quench are measured.
Build-time bundling and framework source generation are outside timed execution;
all measured server processes run without machine-code JIT.

```sh
mkdir -p work
git clone https://github.com/SaltyAom/bun-http-framework-benchmark.git work/web-upstream
git -C work/web-upstream checkout f654bcd175165bce2e3c83b65eaecd8e34294ffd
bun install --frozen-lockfile --cwd work/web-upstream
cargo build --profile production -p quench-node --bin quench-node
cargo build --release -p quench-bench --bin web
```

Use a new directory for each run. `--out` must not exist; its parent must exist.

```sh
cargo run --release -p quench-bench --bin web -- \
  --suite work/web-upstream --out evidence/web-nojit-NEW \
  --quench target/production/quench-node \
  --node node --bun bun --bombardier bombardier --seconds 10 --rounds 3
```

Defaults compare Express, Fastify and Hono on Quench and Node, plus Elysia,
Hono and Fastify on Bun. Select a subset or other upstream frameworks with
`--targets quench/express,node/express,bun/express` (comma-separated).
The runner maps `quench/FRAMEWORK` to `node/FRAMEWORK`; nested targets and
frameworks requiring special build output names are not supported yet.
Use `--seconds 1 --rounds 1` for functional smoke checks, not speed claims.
An iteration build is also diagnostic only. QuickJS has no Node/Bun HTTP host
and is not scored as a web framework runtime.

## Protocol and evidence

- Node uses `--jitless`; preflight retains the explicit flag check. Bun uses
  `BUN_JSC_useJIT=0`; preflight requires the JSC option dump to show
  `useJIT=false`. Quench uses its default interpreted execution path.
- Each server starts alone. An occupied port 3000 aborts that sample; the runner
  never kills unrelated processes. Servers are killed and reaped on completion
  or failure. Startup has a 60-second deadline. External setup/load commands
  have a 180-second deadline (plus five seconds to force termination), so
  `--seconds` is restricted to 1–120.
- Before load, curl checks status, content type, ping text, dynamic query parsing
  and the response header, parsed/serialized JSON equality, and the SHA-256 of
  the full streamed video. Failed correctness means no valid score for that workload; other independently validated workloads may be measured.
- Workloads retain upstream 500 concurrent connections for ping, query and JSON,
  and 10 for video. Video requests force full responses. Bombardier uses its
  fasthttp client with latency reporting. Positive throughput and successful
  requests are required; non-2xx responses and reported errors invalidate a row.
- Round order rotates the framework/runtime list. The load generator runs on the
  same host: short runs and CPU contention can distort absolute throughput.
  Compare matched rows on the same host, framework, workload and bundle hash.
- `final.json` records source revision/status, upstream lock hash, executable
  hashes, versions, JIT proof, effective runtime flags, kernel, cgroup limits,
  toolchain, startup time and raw bombardier results. Per-sample files retain
  server logs, validation bodies/headers and load output. Linux `/proc` status
  before and after load includes `VmRSS` and process peak `VmHWM` in kB; it is
  not a cross-platform process accounting protocol.
- Failed or unavailable combinations remain explicit invalid rows, never zeros
  in an aggregate. `complete=false` and nonzero exit status identify an incomplete
  matrix. Earlier valid rows and failure logs remain available in `report.json`;
  `final.json` is written after the full matrix. No cross-workload average is used.

Upstream package versions include prereleases, so inspect the lock before using
these results to select a production framework. These synthetic HTTP workloads
measure framework overhead, not an authenticated database-backed application.

Lane coordination: [TOD-133](https://linear.app/todoxapp/issue/TOD-133).
