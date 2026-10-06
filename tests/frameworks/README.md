# Express, Koa and Fastify scenarios

These are the Stage A framework targets, using unmodified packages pinned by
`package-lock.json`. Their minimum shared Node behavior is represented by the
18 official upstream fixtures tagged `profile=framework-core` in the single
[`parallel.txt` manifest](../../crates/quench-node-test/node-tests/parallel.txt).
Run the legacy baseline with `run-parallel --profile framework-core`; task 21
will route the same identities through the shared VM. The broader proposals in
tasks 88/89 are superseded.

The profile covers behavior used by the pinned scenarios. The raw
`net.createConnection` pipelining/half-close fixture and the custom
`duplexPair()` HTTP injection fixture remain official Node tests, but exercise
broader socket and transport behavior outside these framework scenarios.

| Scenario | Package |
| --- | --- |
| `scenarios/express.cjs` | express |
| `scenarios/koa.cjs` | koa |
| `scenarios/fastify.cjs` | fastify |

Each scenario serves the same loopback requests from `driver.cjs`: an HTML
`GET /`, a JSON `POST /echo` body, a streamed `GET /stream`, a static
`GET /asset.txt` and a `GET /missing` 404, with gzip accepted.
The driver checks each status and body; for Express it also checks the SHA-1
ETag and the matching conditional `304` response. All three scenarios pass this
oracle on local Node.

Installing pinned packages is setup, not part of the runtime contract. Run the
Node oracle and the shared-VM entry from this directory:

```sh
npm ci
node driver.cjs scenarios/express.cjs
node driver.cjs scenarios/koa.cjs
node driver.cjs scenarios/fastify.cjs
cargo build --profile iteration -p quench-node --bin quench-node-next
../../target/iteration/quench-node-next driver.cjs scenarios/express.cjs
../../target/iteration/quench-node-next driver.cjs scenarios/koa.cjs
../../target/iteration/quench-node-next driver.cjs scenarios/fastify.cjs
```

The `trace` scripts record builtin specifiers and calls made by the three
scenarios under local Node.
