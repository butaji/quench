# npm module, codec, stream and lifecycle repair evidence

Total Node/package/function parity is **not reached**. This packet is not yet
published: source `71db80153` has completed its retained checks, and the new
canonical stream integration is awaiting fresh combined qualification. The performance phase requested after parity has not started.
Instruction screens and load diagnostics never establish package speed wins.

The current frozen source is `71db8015352e6056208f091a67645620226267e7`, executable
SHA256 `90b02c0002b101b2a93706bb27423e80630fac4ff42b372100799d68921ef01d`.
It passes **629 fast and 629 optimized Core tests**, **279 selected Node API
fixtures**, three focused event-iterator fixtures, all four Node host-library
lifecycle tests, and workspace/all-target checks. The decoder and URL corpora
also match on this exact binary. The integration baseline is `791776ba4`,
SHA256 `37a6b7ff031dbcf65c5dd7daf0fbb34fc351811bf3b3af177790526fd2888f60`.

The repair includes module identity/require/cache/parse goals and codecs below,
collection-constructor roots across guest callbacks, callable stream constructor
receivers, child stdin destruction, counted IPC references, and pinned Node
`events.on` policy over Quench's native abort helper. Policy materializes through
the shared lazy data-property mechanism. Ordinary and indexed lazy properties
use the shared integrity operation; array definitions and assignments share
storage after their distinct validation rules. Production code never executes
Node as a fallback. Execa's requested children run `process.execPath`, which is
the selected engine itself.

Follow-up evidence is source-specific. `aeae0c135` passes 626 fast Core and
272 API fixtures; its scenarios retain 273 API / 13 declaration passes, 13 Quench
failures and one platform oracle failure. `68e7ad0f4` retains 626 fast Core,
270/272 API fixtures (two child deadline failures), and scenarios 272 / 13 / 14
plus one platform failure (an additional eslint-plugin timeout during concurrent
heavy jobs). It has 218 unqualified surface matches / 73 differences / nine
oracle failures; callable projections remain 464/70,236, qualified=false.
None of those failures is relabeled. Eager iterator policy loading reproduced
the child deadline failure; lazy materialization restores the unchanged 500 ms
contracts. The final 279-fixture run passes with heavy measurements stopped.

The generic integrity regressions include freeze before first observation,
ordinary/array-index lazy properties across four integrity operations in both
execution modes, accessor replacement on non-extensible arrays, compatible
same-value definitions on frozen arrays, and rejected frozen assignments.
Intermediate failed array probes and interrupted optimized builds remain raw
observations, not completed gates. `snapshots.json` maps historical executables
and distinguishes the earlier fully measured cut from current pending checks.

## Current scenario and callable results

The final candidate has **273 API / 13 declaration passes, 13 Quench failures
and one platform oracle failure**. The exact baseline has **267 / 13 / 19 / one**.
Gains are whatwg-url, execa, scope-manager, loose-envify, jsdom and Testing
Library; there are no scenario regressions. Both retain Lucide-react's 30-second
miss. Surface discovery is **218 unqualified matches / 73 differences / nine
oracle failures**. The callable ledger has **464/70,236 checked paths**,
**69,772 uncovered**, qualified=false. These are finite contract projections.

The current surfaces include a retained generic missing-JSON-import-attribute
error mismatch; it remains in the next repair queue. Node-API addons, Workers,
HTTPS/HTTP2, zlib internals and uncovered function contracts remain open.

## Sources and boundaries

`snapshots.json` binds the official Node 26.12.0 executable, frozen engine
executables, host limits, ICU libraries and the pinned WPT dataset. The initial
optimized candidate is source `59c02ec6b`; `61f150547` additionally includes
CommonJS cache and ambiguous parse-goal repairs. The runner's source fields name
its checkout, which is distinct from the externally selected engine's source;
use the explicit engine mapping in `snapshots.json` for binary provenance.

Repairs use the shared VM and Rust host implementations. There is no production
fallback that executes Node. Module records retain compiled graph identity;
sync require rejects async graphs before effects and permits later async import.
File URL query/fragment identity remains separate from filesystem normalization.
Private field initialization bypasses public Proxy prototypes. CommonJS loading
uses one mutable cache behind `Module._cache`, with captured `require.cache`
references and a private cache for Node-owned library modules. OXC checks both
the CommonJS wrapper and ESM syntax for ambiguous files.

## Retained observations

- Initial candidate: all **300** scenario outcomes retained: **271 API passes,
  13 declaration passes, 15 Quench failures, one platform oracle failure**.
  whatwg-url, jsdom and Testing Library pass. Lucide-react misses the 30-second
  deadline. A subsequent matched longer diagnostic completes on both baseline
  (31.59s) and candidate (31.38s) with identical successful output. This does not
  establish a draft regression or qualify the missed 30-second deadline.
- whatwg-url: **126 public API assertions** and matching, still-unqualified API
  discovery. The pinned WPT corpus has **896 cases**; the Node package matches
  its expected fields, and Quench matches all captured Node results, including
  public methods and lower-level parse/serialize outcomes.
- Complete-buffer TextDecoder: **896 mixed cases**, **19,968 single-byte cases**,
  and **1,310,720 two-byte cases** match Node exactly. The latter enumerates every
  byte pair with fatal mode off/on for ten Unicode/legacy encodings. Both streams
  have 5,120 data rows and an explicit completion record. GB18030/GBK U+FEFF
  retention under both ignoreBOM settings is separately retained.
- Initial Core snapshot: **620 fast and 620 optimized tests** pass. After merging
  integration `5858b2949` and fixing realm reset: **622 fast and 622 optimized
  tests** pass. This does not qualify subsequent runtime integration `ae439807e`.
- **Nine focused CommonJS fixtures** pass on `61f150547` with Node 26.12 as oracle.
  The new cache and syntax-detection fixtures check explicit completion and exact
  output equality. All four Node library tests pass, including cache-root GC and
  VM reuse with the public Module cache owner.
- Workspace/all-target checks pass at `61f150547`; compilation without ICU also
  passes. These are build checks, not full API qualification.

The VM reuse panic reproduces on both clean integration `5858b2949` and the draft.
Retained backtraces reach stale startup-intrinsic atom ids during a second
bootstrap. `9aafdebbe` discards the old snapshot before heap reset and forgets the
draft's Node module roots. The regression exercises both execution modes.
`superseded-core-run.json` records why an earlier optimized run was interrupted;
it is not counted as a pass.

## Replay

Install the pinned `tests/npm-top300` dependencies. Set `NODE` to the exact Node
oracle and `QUENCH` to an optimized engine built with ICU available to pkg-config.
For example, Debian's ICU development package supplies the standard metadata.
The measured host uses ICU 76.1; Node includes ICU 78.3. Tables outside this
retained corpus, streaming state, coercions and metadata remain obligations.

From the repository root, with `PACKET=evidence/npm-esm-codecs-2026-10-15`:

```sh
"$NODE" "$PACKET/decoder-two-byte.cjs" > node.ndjson
"$QUENCH" "$PACKET/decoder-two-byte.cjs" > quench.ndjson
cmp node.ndjson quench.ndjson
"$NODE" "$PACKET/url-wpt.cjs" "$PACKET/urltestdata.json" "$PWD/tests/npm-top300/node_modules/whatwg-url" > node-url.json
"$QUENCH" "$PACKET/url-wpt.cjs" "$PACKET/urltestdata.json" "$PWD/tests/npm-top300/node_modules/whatwg-url" > quench-url.json
cmp node-url.json quench-url.json
```

The mixed and single-byte decoder guests replay the same way. Raw compressed
outputs, failure backtraces and test logs retain their actual outcomes. WPT data
comes from commit `ad450c89a7ba1c42597b5c899d7ba9e309b9e90c`; its license is included.
The original absolute-path WPT guest is retained alongside the portable guest.

## Portable Execa replays

The portable guests take the installed package and child fixture paths as data.
Compare Node and Quench stdout and require `complete:true`. The public corpus
has 13 result rows; the IPC corpus exercises sendMessage, getOneMessage,
getEachMessage and getCancelSignal, plus execaNode. These finite cases do not
qualify every input or returned/dynamic method.

```sh
"$NODE" "$PACKET/execa-public-api-portable.mjs" "$PWD/tests/npm-top300/node_modules/execa" "$PACKET/execa-node-child.cjs"
"$QUENCH" "$PACKET/execa-public-api-portable.mjs" "$PWD/tests/npm-top300/node_modules/execa" "$PACKET/execa-node-child.cjs"
"$NODE" "$PACKET/execa-ipc-parent-portable.mjs" "$PWD/tests/npm-top300/node_modules/execa" "$PACKET/execa-ipc-child-portable.mjs"
"$QUENCH" "$PACKET/execa-ipc-parent-portable.mjs" "$PWD/tests/npm-top300/node_modules/execa" "$PACKET/execa-ipc-child-portable.mjs"
```

`qualification-suites.py` retains the workspace-specific serial orchestration:
optimized Core completion, Node host library tests, corpus comparisons, exact
baseline build, candidate/baseline scenarios, surface/coverage, all-eight
Callgrind instructions and 32 independent exact completion checks. The fixed
30-second package deadlines are unchanged. Compile and measurement stages run
serially. The interpreter is used throughout; no package speed claim is made.

The two-byte validator initially used Python Unicode `splitlines`, which split
JSON strings containing literal U+0085/U+2028/U+2029. Both engine outputs were
already byte-identical with 5,121 LF-delimited records and the explicit
1,310,720-case completion. The recorded validator failure is preserved; the
corrected check uses LF boundaries and revalidates retained successful outputs
and exact frozen executable hashes before resuming. It never changes guest data.

The completed `71db80153` instruction screen retains all eight benchmarks and
32 independent exact completion checks. The largest per-run change is
**+0.1072% (raytrace)** against `791776ba4`;
startup counts are retained separately. There is no new >1% per-run finding.
This screen compares Quench cuts and makes no package speed claim.

Integration `fcc81c6a0` subsequently introduced canonical Node stream algorithms
and runtime/encoding changes. Merge `2632a993e` retains the owner's canonical
implementation, removes the superseded draft constructor adapter, preserves
both API exports and error helpers, and keeps the private Node-library cache
authoritative while retaining live module.exports behavior. Its fresh combined
checks remain pending. Results on `71db80153` do not qualify this new merge.
