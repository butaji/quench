# Web-framework scenarios

Real, unmodified web frameworks pinned by `package-lock.json`. They define the
deferred framework investigation ([task 88](../../tasks/88.md) and
[task 89](../../tasks/89.md)). It adds no gate to the current two-stage plan.

| Scenario                | Packages                                                   |
| ----------------------- | ---------------------------------------------------------- |
| `scenarios/express.cjs` | express                                                    |
| `scenarios/koa.cjs`     | koa                                                        |
| `scenarios/fastify.cjs` | fastify                                                    |
| `scenarios/hono.cjs`    | hono, @hono/node-server                                    |
| `scenarios/nest.cjs`    | @nestjs/core, @nestjs/common, @nestjs/platform-express     |
| `scenarios/h3.cjs`      | h3 (the Nuxt/Nitro server core)                            |
| `scenarios/next.cjs`    | next, react, react-dom (production server for `next-app/`) |

Every scenario serves the same loopback request set from `driver.cjs`: an HTML
`GET /`, a JSON `POST /echo` body, a streamed `GET /stream`, a static
`GET /asset.txt` and a `GET /missing` 404, with `Accept-Encoding: gzip`.

## Setup

Installation and the Next.js production build are setup steps, not part of the
runtime contract. `next build` uses a native SWC addon and runs only under
Node; Quench runs the prebuilt server.

```sh
cd tests/frameworks
npm ci
(cd next-app && ../node_modules/.bin/next build)
node driver.cjs scenarios/express.cjs
```

## Tracing

The inventory derivation runs each scenario under local Node with:

- `trace/requests.cjs`: builtin specifiers each package requests (`--require`,
  writes `OUT`);
- `trace/calls.cjs`: builtin exports and web globals that framework or
  scenario code calls (`--require`, writes `OUT`).

The scenarios contain no Quench-specific code and must keep matching local
Node exactly.
