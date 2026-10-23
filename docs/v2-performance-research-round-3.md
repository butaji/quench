# 100 fresh performance experiments for TOD-173

This third research pass was checked against both earlier backlogs, the React
results and rejected screens in `tests/npm-perf/README.md`, and Linear TOD-173
and its comments. The items below are new hypotheses and workload gaps; none is
a measured win. They are ordered for coverage, not by a claim that each will
help. Several package entries deliberately add workload dimensions so the
runtime bottleneck can be identified before changing code.

Every shortlisted change must preserve output and error behavior, then be
measured with matched inputs on Quench, Node, and Bun in both no-JIT and JIT
modes. Record throughput and maximum RSS independently. Linux RSS samples are
diagnostic only; use the repository's qualification host and method for wins.
TOD-173 currently has a timed React SSR workload, while the other nine package
workloads remain to be added.

## React and react-dom

1. **Include tree construction in a separate workload.** Build the same
   element tree on every timed render and compare with the existing reused-tree
   case; optimize object/array construction only if it materially affects
   end-to-end renders/s and max RSS.
2. **Exercise shallow and deep function-component trees.** Add many tiny
   component calls and longer call chains, then target Quench activation,
   return, and call-window setup where costs per node are highest.
3. **Compare static markup output.** Run `renderToStaticMarkup` on the same
   tree; use its distinct code path to find a smaller or faster output path
   without conflating it with hydration markup. [React DOM server APIs](https://react.dev/reference/react-dom/server)
4. **Separate attribute escaping from child-text escaping.** Use repeated
   values containing quotes, apostrophes, and ampersands; profile attribute
   conversion and copies, then specialize common safe spans if they dominate.
5. **Specialize boolean and overloaded-boolean attributes.** Exercise
   `disabled`, `download`, `capture`, `data-*`, and `aria-*`; use profile
   evidence to reduce repeated property classification at stable serialization
   sites.
6. **Add form-element SSR cases.** Cover `textarea`, `select`, and `option`
   with controlled/default values, then optimize their special prop lookup and
   value conversion separately from ordinary host tags.
7. **Add SVG and namespace-transition cases.** Profile nested SVG tags and
   attributes; cache or specialize namespace decisions only where namespace
   identity and mutation semantics permit it.
8. **Compare monomorphic and polymorphic element shapes.** Alternate stable
   and varied tag/prop shapes, count field-cache hit and fallback rates, and
   tune cache polymorphism to avoid megamorphic lookup cost.
9. **Reduce context lookup through deep component trees.** Add nested
   providers and repeated consumers, then optimize context stack access only
   if it is a material part of per-node rendering cost.
10. **Measure tree identity changes without output reuse.** Compare stable,
    fresh-but-equivalent, and mutated trees; use the result to tune allocation
    and shape caches while ensuring each render observes current props.

## Next

11. **Compile route manifests into a static/dynamic decision tree.** Sweep
    large manifests with static, parameter, and catch-all routes; target
    repeated route matching while preserving precedence. [Dynamic routes](https://nextjs.org/docs/app/building-your-application/routing/dynamic-routes)
12. **Compile middleware matchers by predicate.** Test high-fanout path and
    header matcher sets; build a decision path that rejects early and avoids
    allocating intermediate match arrays. [Middleware matchers](https://nextjs.org/docs/app/api-reference/file-conventions/middleware#matcher)
13. **Pre-size static-param generation storage.** Sweep thousands of
    `generateStaticParams` results and reserve arrays/maps from known
    cardinality to reduce growth copies and unused retained capacity. [generateStaticParams](https://nextjs.org/docs/app/api-reference/functions/generate-static-params)
14. **Optimize `Response.json` route handlers.** Compare it with equivalent
    constructed and streamed responses; target JSON/string/buffer conversion
    and response-body setup separately. [Route Handlers](https://nextjs.org/docs/app/getting-started/route-handlers)
15. **Parse cookies lazily per request.** Compare no cookie access, one
    lookup, and enumeration; cache a request-local parse only after the first
    read and bound its retained storage. [cookies](https://nextjs.org/docs/app/api-reference/functions/cookies)
16. **Reduce nested request-context lookup work.** Benchmark repeated
    `headers()`/`cookies()` access from nested server components and optimize
    async-context entry or request-local lookup only if it is independently
    hot.
17. **Use a compact Data Cache key representation.** Separate hits, misses,
    and revalidation; hash structured key components incrementally rather than
    building a temporary serialized key when semantics allow. [Data fetching and caching](https://nextjs.org/docs/app/getting-started/fetching-data)
18. **Bound stale-response retention during ISR.** Compare fresh, stale, and
    expired requests under controlled regeneration; reduce duplicate retained
    response bodies while preserving stale-while-revalidate behavior. [ISR](https://nextjs.org/docs/app/guides/incremental-static-regeneration)
19. **Parse request bodies from byte views.** Test JSON, URL-encoded, and
    streamed sizes; remove intermediate string or Buffer copies when the
    request API permits direct byte parsing. [Route Handler request bodies](https://nextjs.org/docs/app/getting-started/route-handlers#request-body)
20. **Bound response queues under consumer backpressure.** Compare fast and
    paused stream consumers; avoid excess queued chunks and microtasks while
    measuring completed responses/s and peak retained bytes.

## Vite

21. **Reduce cold dependency-scan allocations.** Use large shared import
    graphs and many entries; profile parse products and directory-scan
    collections, then reuse or compact transient representations.
22. **Specialize CommonJS interop detection.** Vary export patterns in the
    dependency optimizer; share repeated detection results across modules by
    file identity and optimizer generation.
23. **Resolve linked packages with a bounded realpath cache.** Compare
    preserved and canonical symlink behavior in workspaces; cache canonical
    paths with invalidation on filesystem changes.
24. **Reduce dynamic-import preload rewrite work.** Use repeated shared
    dependency subgraphs and build a compact dependency list without repeated
    flattening or string concatenation. [Vite build](https://vite.dev/guide/build.html)
25. **Avoid rebuilding module-preload dependency lists.** Compare default
    resolution with custom `resolveDependencies`; reuse immutable lists across
    chunks when their graph generation is unchanged. [Vite build](https://vite.dev/guide/build.html)
26. **Deduplicate extracted CSS across entry points.** Build overlapping
    CSS graphs and intern identical emitted chunks before materializing output
    strings. [CSS code splitting](https://vite.dev/guide/features.html#css-code-splitting)
27. **Share worker module-graph nodes.** Build many `?worker` and
    `?sharedworker` entries with common imports; use the same immutable graph
    nodes and avoid duplicate traversal state. [Web workers](https://vite.dev/guide/features.html#web-workers)
28. **Stream manifest serialization.** Build a large multi-page manifest and
    compare materializing one giant object/string with incremental output;
    measure output parity, allocations, and RSS. [Build manifest](https://vite.dev/config/build-options.html#build-manifest)
29. **Merge `.env` files directly into their precedence map.** Avoid temporary
    entry arrays and duplicate maps during dotenv expansion; preserve the
    documented override order. [Vite env implementation](https://github.com/vitejs/vite/blob/main/packages/vite/src/node/env.ts)
30. **Fuse import URL cleanup and extension correction.** Scan path, query,
    and extension once while preserving virtual IDs; measure temporary strings
    and transform throughput. [Vite module graph](https://github.com/vitejs/vite/blob/main/packages/vite/src/node/server/moduleGraph.ts)

## esbuild

31. **Use virtual stdin to avoid host file setup.** Compare `stdin.contents`
    with equivalent files and optimize wrapper argument construction and
    resolution only where JS overhead is visible. [stdin](https://esbuild.github.io/api/#stdin)
32. **Specialize the serve response path.** Repeatedly serve one fixed bundle
    locally; reduce JS response-object overhead and measure it separately
    from Go service work. [serve](https://esbuild.github.io/api/#serve)
33. **Compile external-package patterns.** Stress many imports with large
    `external` lists; pre-tokenize patterns and avoid repeating prefix checks
    per import. [external](https://esbuild.github.io/api/#external)
34. **Specialize loader outputs for data URLs and files.** Sweep asset sizes
    around the data-URL threshold; avoid duplicate base64 strings and emitted
    file records. [Loaders](https://esbuild.github.io/content-types/#base64)
35. **Compile output naming templates.** Generate many chunks with `[dir]`,
    `[name]`, `[hash]`, and extension substitutions; parse each template once
    and reuse its fixed segments. [Entry names](https://esbuild.github.io/api/#entry-names)
36. **Extract legal comments in one pass.** Compare all legal-comment modes
    on large comment-heavy graphs; retain comment ranges instead of repeatedly
    slicing source strings. [Legal comments](https://esbuild.github.io/api/#legal-comments)
37. **Reuse banner/footer fragments across chunks.** Build many entry points
    with identical text and append shared immutable fragments during output
    assembly. [Banner and footer](https://esbuild.github.io/api/#banner)
38. **Pre-resolve injected modules once per build graph.** Compare multiple
    injected modules and entrypoints; share their resolved records instead of
    repeating path resolution. [Inject](https://esbuild.github.io/api/#inject)
39. **Defer diagnostic formatting until emission.** Generate many warnings
    and use log filters/limits; store compact diagnostic fields and format only
    messages that will actually be emitted. [Logging](https://esbuild.github.io/api/#logging)
40. **Normalize paths once at the API boundary.** Compare relative and
    absolute entries under varied working directories; carry canonical path
    IDs through resolution and output planning. [absWorkingDir](https://esbuild.github.io/api/#abs-working-dir)

## Fastify

41. **Flatten encapsulated hook chains at route registration.** Deeply nest
    plugins, compile the inherited hook sequence into the route, and remove
    per-request scope walks. [Encapsulation](https://fastify.dev/docs/latest/Reference/Encapsulation/)
42. **Normalize parser content types once.** Precompute a route's fixed
    content-type key and retain a general path for parameters and charset
    variants. [Content-type parser](https://fastify.dev/docs/latest/Reference/ContentTypeParser/)
43. **Write already-serialized responses directly.** Compare string/Buffer
    replies with JSON-schema objects and avoid serializer setup when the body
    is already serialized. [Reply source](https://github.com/fastify/fastify/blob/main/lib/reply.js)
44. **Compile a distinct injection request lifecycle.** Separate `inject()`
    from socket handling and optimize the in-memory request path without
    constructing transport-only state. [Testing and injection](https://fastify.dev/docs/latest/Guides/Testing/)
45. **Keep request/reply initialization order stable.** Vary decorators and
    hook sets; reduce shape transitions with predictable property layout and
    verify caches under each route shape. [Request](https://fastify.dev/docs/latest/Reference/Request/), [reply](https://fastify.dev/docs/latest/Reference/Reply/)

## Undici

46. **Fast-path direct `Dispatcher.dispatch`.** Compare it with `request()`
    over the same pool and payload; reduce wrapper setup and callback
    allocations where the lower-level API is used. [Dispatcher](https://undici.nodejs.org/#/docs/api/Dispatcher)
47. **Tune pool pipelining from workload shape.** Sweep depth, payload, and
    concurrency against a fixed local server; select the fastest setting under
    an explicit retained-connection memory cap. [Pool](https://undici.nodejs.org/#/docs/api/Pool)
48. **Avoid stream adapter churn.** Compare Web Stream and Node stream body
    consumption; reuse adapter state and avoid chunk copies where ownership
    permits. [Undici body mixins](https://github.com/nodejs/undici#body-mixins)
49. **Reuse immutable request options safely.** Compare reused and fresh
    options, then cache normalized option records by object identity only when
    they remain unchanged. [Client](https://undici.nodejs.org/#/docs/api/Client)
50. **Share keep-alive timer state per pool.** Under steady traffic, reduce
    expiry timer churn and measure idle socket count, timers, throughput, and
    RSS. [Client](https://undici.nodejs.org/#/docs/api/Client)

## ws

51. **Cache fixed WebSocket upgrade prefixes.** For a fixed endpoint and
    protocol list, reuse constant accept-hash input while keeping validation
    dynamic. [WebSocketServer](https://github.com/websockets/ws/blob/master/lib/websocket-server.js)
52. **Reuse outbound Buffer payload storage when safe.** Compare Buffer and
    string echo and avoid redundant payload copies when ownership and masking
    rules allow. [Sender](https://github.com/websockets/ws/blob/master/lib/sender.js)
53. **Cork small header/payload writes together.** Sweep small frames and
    uncork before yielding; compare writes/syscalls, throughput, and buffered
    memory. [Sender](https://github.com/websockets/ws/blob/master/lib/sender.js)
54. **Reject oversized frames before buffering payloads.** Validate the
    decoded length against `maxPayload` before copying; compare valid and
    rejected traffic for bytes copied and RSS. [Receiver](https://github.com/websockets/ws/blob/master/lib/receiver.js)
55. **Carry UTF-8 validator state across fragments.** Validate fragmented
    text incrementally instead of concatenating and rescanning the whole
    message. [Receiver](https://github.com/websockets/ws/blob/master/lib/receiver.js)

## Pino

56. **Write default timestamps from the numeric clock value.** Avoid an
    intermediate date string and compare custom timestamp callbacks as a
    fallback. [Time implementation](https://github.com/pinojs/pino/blob/main/lib/time.js)
57. **Cache encoded level labels.** Reuse fixed JSON bytes for each level and
    avoid per-record string conversion. [Pino API](https://github.com/pinojs/pino/blob/main/docs/api.md)
58. **Represent child bindings as a persistent chain.** Benchmark child
    creation and repeated log calls; emit parent bindings during serialization
    rather than copying each child object eagerly. [Child loggers](https://getpino.io/#/docs/api?id=child)
59. **Serialize once for multi-target transports.** Fan out an immutable
    encoded record rather than formatting separately per output target.
    [Transports](https://getpino.io/#/docs/transports)
60. **Remove `logMethod` dispatch from the no-hook path.** Add a direct common
    logger path when no hook is configured and preserve the generic hook path
    for configured loggers. [Hooks](https://getpino.io/#/docs/hooks)

## sharp and Quench Node-API

61. **Encode N-API UTF-8 directly into the provided destination.** Remove the
    intermediate UTF-16/runtime-string conversion in
    `napi_get_value_string_utf8`; preserve truncation and surrogate semantics.
    [Node-API](https://nodejs.org/api/n-api.html#napi_get_value_string_utf8)
62. **Construct runtime strings directly from valid N-API UTF-8.** Avoid the
    temporary `Cow<str>` conversion in `napi_create_string_utf8`, with a
    compatibility fallback for malformed sequences. [Node-API](https://nodejs.org/api/n-api.html#napi_create_string_utf8)
63. **Add a direct Latin-1 string construction path.** Avoid the temporary
    `Vec<u16>` in `napi_create_string_latin1` and verify every byte value.
    [Node-API](https://nodejs.org/api/n-api.html#napi_create_string_latin1)
64. **Use inline storage for callback argument roots.** In the host-to-guest
    N-API argument path, keep common arities in fixed inline storage and fall
    back to a vector for larger calls. [Node-API calls](https://nodejs.org/api/n-api.html#napi_call_function)
65. **Store stable callback records directly in host-function metadata.**
    Remove external-address resolution from each native callback trampoline;
    measure calls/sec and retain the generic validation path for invalid data.
66. **Avoid zero-initializing buffers before `napi_create_buffer_copy`.**
    Allocate unpublished storage, copy bytes once, then expose it as a Buffer;
    keep zero-fill semantics for `napi_create_buffer`. [Node-API buffers](https://nodejs.org/api/n-api.html#napi_create_buffer_copy)
67. **Return a fresh Buffer and backing storage together.** Avoid constructing
    the Buffer and then resolving its storage in a second runtime operation.
68. **Dispatch typed-array brand checks directly.** Replace repeated linear
    enum-table searches with an exhaustive brand match and preserve all
    supported brands. [Node-API typed arrays](https://nodejs.org/api/n-api.html#napi_get_typedarray_info)
69. **Skip handle-scope truncation if the scope added no handles.** Avoid a
    mutable borrow on native callback return unless its handle length changed.
70. **Remove redundant environment pointers from completion events.** Store
    the `AsyncWork` pointer alone and access its live environment when draining.
71. **Avoid cloning the environment event-queue `Arc` per async task.** Read
    the stable queue through the owning environment while preserving the
    N-API worker/completion thread rules. [Asynchronous work](https://nodejs.org/api/n-api.html#asynchronous-operations)
72. **Replace the worker-pool receiver mutex handoff.** Compare a true
    multi-consumer queue or condition variable with the current serialized
    blocking receive path under CPU-heavy sharp jobs. [Rust mpsc](https://doc.rust-lang.org/std/sync/mpsc/)
73. **Use a compact N-API async-work queue item.** Avoid a boxed trait-object
    closure on this frequent queue path, retaining the generic closure
    fallback.
74. **Split hot callback fields from cold metadata.** Measure `CallbackRecord`
    access locality and separate rarely used signature/class fields only if it
    reduces cache misses without raising retained RSS.
75. **Compare sharp input representations at equal pixels.** File,
    encoded-Buffer, and raw-pixel inputs isolate conversion cost before changing
    buffer paths. [sharp constructor](https://sharp.pixelplumbing.com/api-constructor/)
76. **Handle Buffer views at non-zero offsets without full-store copies.**
    Exercise sliced input views and verify byte offset/length behavior while
    measuring allocations and throughput. [sharp constructor](https://sharp.pixelplumbing.com/api-constructor/)
77. **Use callback completion as a separate async path.** Compare identical
    sharp pipelines through callback and Promise APIs; reduce wrapper work
    without changing completion ordering. [sharp output](https://sharp.pixelplumbing.com/api-output/)
78. **Stream output with bounded high-water marks.** Sweep readable/writable
    queue limits for fixed images; reduce retained native and guest chunks
    while preserving backpressure. [sharp output](https://sharp.pixelplumbing.com/api-output/)
79. **Avoid duplicate conversions for metadata-only image work.** Benchmark
    JPEG, PNG, WebP, and AVIF metadata paths with and without EXIF/ICC data;
    optimize wrapper parsing after separating libvips time. [sharp input](https://sharp.pixelplumbing.com/api-input/)
80. **Keep fluent sharp chains in one optimized call path.** Sweep chain
    length with equal final output and target N-API callback/host-call
    dispatch overhead if it dominates. [sharp resize](https://sharp.pixelplumbing.com/api-resize/)

## Zod

81. **Allocate async continuation arrays only after a Promise appears.**
    Exercise successful sync object, array, tuple, record, Map, and Set parsing
    and measure allocations per parse. [Zod schemas](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts)
82. **Handle tuple results in one synchronous pass.** Avoid the temporary
    `itemResults` array when tuple children are synchronous; compare valid and
    invalid tuples by size. [Tuple parser](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L3040)
83. **Iterate tuple rest elements without slicing.** Read by index from the
    original input after the fixed prefix and measure copied bytes and RSS.
84. **Avoid entry-pair arrays while parsing Maps.** Specialize built-in Map
    iteration with fallback for modified iterators; measure allocations per
    entry. [Map parser](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L3473)
85. **Pre-size parsed Maps from input size.** Measure resize work and retained
    capacity across short and long Map inputs.
86. **Pre-size parsed Sets from input size.** Preserve post-transform
    deduplication and insertion order while reducing growth copies.
87. **Track XOR matches in scalar locals.** Keep the first successful result
    and count matches without a temporary array; materialize detail only for
    failure construction.
88. **Fast-path intersections of disjoint plain objects.** Merge without
    recursive shared-value reconciliation when schema metadata proves keys
    disjoint. [Intersection merge](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L2859)
89. **Replace repeated linear key searches in intersection merging.** Compare
    a membership index or sorted merge across object widths and account for
    temporary memory.
90. **Allocate intersection error maps lazily.** Initialize error structures
    only on the first relevant issue; benchmark valid and invalid cases.
91. **Cache declared record-key sets per schema.** Avoid rebuilding enum or
    literal key membership structures on repeated parses, with invalidation
    if schema metadata is mutable. [Record parser](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L3270)
92. **Scan numeric record keys directly.** Replace regex plus `Number()` with
    a parser for exactly accepted decimal forms and verify signs, exponents,
    and leading-zero behavior.
93. **Use a single-child async Map path.** Avoid allocating and awaiting a
    two-element `Promise.all` result when only the key or value parser is
    asynchronous. [Map async parser](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L3496)
94. **Inline the `z.pipe` issue scan.** Replace the `.some()` callback with an
    early-exit loop and a direct empty-issues fast path. [Pipe parser](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L4483)
95. **Use a compact record-key membership structure for small schemas.**
    Compare fixed tables with `Set.has` across schema size and key type while
    preserving symbol behavior.
96. **Fast-path native Promises in `z.promise`.** Attach directly to Quench
    native Promises; preserve `Promise.resolve` semantics for thenables.
    [Zod Promise parser](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L5034)
97. **Guard Set insertion in transform-heavy parsing.** Specialize ordinary
    built-in Set output while preserving transformed-value deduplication and
    insertion order.
98. **Skip `Object.freeze` for primitive readonly results.** Keep the object
    path unchanged and measure scalar schemas and arrays. [Readonly parser](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L4659)
99. **Avoid Promise wrapping for synchronous transforms in async parsing.**
    Return the synchronous result directly while retaining the public async
    entry point's Promise contract. [Transform parser](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L3871)
100. **Stream intersection issue reconciliation.** Avoid building and then
     filtering intermediate `bothKeys` arrays; preserve issue order and
     aggregation semantics. [Intersection issues](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts#L2919)

## Research anchors

## Foundational leverage points

The package list is not the most useful optimization order. The React result
shows an interpreter throughput gap of roughly 4.8x even after targeted opcode
fusion and workload-weighted PGO; another small dispatch fusion cannot close
that gap alone. The following cross-cutting changes attack bytes moved,
allocations, or repeated interpretation and could benefit several packages.
They are separate architecture hypotheses, not counted in the 100 above.

- **Narrow common object-property storage.** `Value::to_narrow` can encode
  common values in 32 bits, but `ValueArena` stores object properties as 64-bit
  `Value`s. Measure the narrowable fraction, then prototype 32-bit blocks with
  a sparse wide-value sidecar. It may reduce the object working set for React,
  Fastify, Pino, and Zod. Use a guarded representation so wide values preserve
  identity and behavior. This follows the memory principle behind [V8 pointer
  compression](https://v8.dev/blog/pointer-compression).
- **Region-allocate short-lived graphs and promote escapes.** React element
  trees and request-local framework state are often short-lived. Allocate a
  group together, then promote values that escape through closures, returns,
  or host calls. Record promotion rate before implementing; identity,
  mutation, and GC reachability make this a high-risk change. Related work:
  [optimistic stack allocation with dynamic heapification](https://doi.org/10.1145/3656389).
- **Move rare, large cell payloads out of the common slot.** Measure the
  current `Cell` size and variant frequency, then place cold payloads in
  side tables so ordinary object/array/string cells pack more densely. Check
  that indirection on uncommon variants does not erase the cache and RSS win.
- **Stream rope leaves through consumers.** Quench already has flat and rope
  strings. Let HTML escaping and JSON serialization read code units from a
  cursor across leaves rather than flattening first; measure bytes copied,
  temporary allocation, depth, and output throughput. Balance joins only if
  observed rope depth also makes cursor traversal expensive.
- **Treat no-JIT as a separate compile-time model decision.** Explore an
  install-time/static baseline artifact for pinned package bytecode, with
  dynamic evaluation falling back to the interpreter. This can eliminate
  runtime compile cost but changes the no-JIT model, so it needs an explicit
  apples-to-apples qualification rule and startup/code-size/RSS accounting.

The architectural ranking is: first quantify narrow-value coverage and
temporary-object lifetimes; next reduce string flatten/copy boundaries; then
evaluate the rare-cell layout. The existing React and PGO measurements should
be the first before/after workloads. If native work dominates (notably sharp or
esbuild), runtime changes cannot create a large package-level speedup until
the JS wrapper or host-crossing share is measured.

- React and Next: [React server APIs](https://react.dev/reference/react-dom/server), [Next route handlers](https://nextjs.org/docs/app/getting-started/route-handlers), and [Next response caching](https://github.com/vercel/next.js/blob/canary/packages/next/src/server/response-cache/index.ts).
- Vite: [dependency optimization](https://vite.dev/guide/dep-pre-bundling.html), [build](https://vite.dev/guide/build.html), and [plugin API](https://vite.dev/guide/api-plugin.html).
- esbuild: [API](https://esbuild.github.io/api/) and [content types](https://esbuild.github.io/content-types/).
- Fastify: [encapsulation](https://fastify.dev/docs/latest/Reference/Encapsulation/) and [reply](https://github.com/fastify/fastify/blob/main/lib/reply.js); Undici: [Pool API](https://undici.nodejs.org/#/docs/api/Pool); ws: [source](https://github.com/websockets/ws); Pino: [transports](https://getpino.io/#/docs/transports).
- sharp: [constructor](https://sharp.pixelplumbing.com/api-constructor/), [output](https://sharp.pixelplumbing.com/api-output/), and [metadata](https://sharp.pixelplumbing.com/api-input/); Node-API: [reference](https://nodejs.org/api/n-api.html).
- Zod: [v4 schema implementation](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts) and [parse entry points](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/parse.ts).
- Project gate and attempted results: [TOD-173](https://linear.app/todoxapp/issue/TOD-173/win-ten-popular-npm-package-workloads-across-node-and-bun-jit-modes), [`tests/npm-perf/README.md`](../tests/npm-perf/README.md), and both previous research docs: [`v2-performance-experiments.md`](v2-performance-experiments.md), [`v2-performance-research-round-2.md`](v2-performance-research-round-2.md).
