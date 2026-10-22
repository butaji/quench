# Published npm compatibility and callable coverage

This suite pins 300 published packages against latest Node. The target is public
function parity, tracked in [TOD-127](https://linear.app/todoxapp/issue/TOD-127).
It is not complete. Representative scenarios, descriptor discovery, declaration
consumers and upstream tests are separate evidence; none alone qualifies a package.

`inventory.json` retains versions, tarball integrities, download counts and sources.
The ranking is current npm weekly downloads among 4,665 candidates discovered by
273 popular-package seed queries, not an exhaustive registry census. Raw counts
and five query exceptions are in
`evidence/npm-top100-2026-10-14/ranking-source-current.json`. The original and
refreshed 100-package baselines remain archived in that evidence directory.

The locked shared install includes a supported TypeScript 6 peer context for the
ESLint packages. The selected TypeScript 7 package is installed under the explicit
`typescript-under-test` alias; its inventory entry owns that mapping, used by the
runner, discovery and compiler scenarios. This prevents the unsupported TS7/ESLint
combination from being misreported as a runtime failure. Other latest-version peer
conflicts remain visible in npm's install observations.

The inventory includes native Linux glibc and musl artifacts. `--force` permits
both to be retained in the lockfile; it does not make a musl addon runnable on
glibc. Node oracle failures and platform gaps remain qualification blockers.
Native addons need Quench's Node-API support; native CLI checks test host launches.

```sh
cd tests/npm-top300
npm ci --ignore-scripts --force --no-audit --no-fund
```

From the repository root, use absolute engine/report paths and a positive
per-package deadline:

```sh
cargo build --profile iteration -p quench-node --bin quench-node
cargo build --profile iteration -p quench-node-test --bin run-npm --bin npm-coverage
target/iteration/run-npm /path/to/node /path/to/quench-node /path/to/scenarios.json 30
target/iteration/run-npm /path/to/node /path/to/quench-node /path/to/surface.json 30 --surface
target/iteration/npm-coverage /path/to/surface.json /path/to/scenarios.json /path/to/coverage.json
```

Append an exact package name to either engine run for a focused check. Each
process has a separate temporary working directory, process group and deadline.
Reports are checkpointed after every package and retain source revision/dirty
state, executable hashes, Node version, inventory/lock/fixture hashes, status,
signal, timeout, stdout and stderr. Text output is stored as UTF-8; non-UTF-8 output
retains its bytes. A scenario pass requires exact success output and empty stderr.
Import errors, crashes, timeouts, oracle failures and uncovered scenarios never
count as passes. Declaration consumers are reported separately.

`surface.mjs` traverses own descriptors and package/class prototypes, including
symbol methods and accessors, across static and expanded pattern exports and
import/require condition branches. It avoids invoking getters during discovery.
CLI exports listed as package binaries are recorded separately. It explicitly
retains load failures, unresolved patterns, and missing coverage of returned or
dynamically installed APIs. Discovery executes package initialization in an
isolated process; it supplies an observed surface, not a static completeness proof.

Surface matches are always `surface-match-unqualified` and the surface command
returns nonzero. `npm-coverage` pairs reports only when inventory, lockfile and
engine hashes agree. It derives per-callable obligations from Node's descriptors
and successful, matching `function-checks.json` contract artifacts. A scenario
without function tags supplies zero function coverage. Checked paths mean the
retained input corpus ran on both engines, not that every input is proven equal.
The coverage gate returns nonzero while whole-package obligations remain open.

The semver contract uses unchanged upstream v7.8.5 fixture data plus focused
contracts for root functions, class methods and accessors. Its source commit and
ISC license are retained under `contracts/upstream-semver/`; one constants adapter
routes the fixture to the pinned published implementation. This is separate from
the full upstream TAP suite, whose Node and Quench observations are retained in
the 300-package evidence. Other package scenarios still cover small API subsets.

The older `tests/npm-popular` suite retains its own pinned versions and filesystem
regressions. This inventory extends it; neither suite implies complete npm parity.
