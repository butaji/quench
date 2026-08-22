# Quench Runtime Goal

Build a portable JavaScript runtime for Node-compatible applications by
separating JavaScript engine semantics from Node compatibility semantics.
The finish line is the **complete** Node API test suite running green on
the sole quench-runtime implementation.

## Architecture

The system is layered from general to specific:

1. **JavaScript runtime** — values, objects, functions, promises, execution,
   and engine semantics.
2. **Platform capabilities** — I/O, clocks, tasks, processes, cryptography,
   randomness, and other host primitives.
3. **Resources and operations** — generic resource identity, lifecycle,
   synchronous and asynchronous operations, cancellation, and completion.
4. **Protocols** — bytes, streams, events, backpressure, promises, and IPC.
5. **Node facade** — Node-specific modules, validation, errors, exports,
   compatibility behavior, and API declarations.

Everything externally stateful should be modeled as a resource. Everything
that happens to a resource should be modeled as an operation. Composable
behavior should be expressed as protocols. Node-specific quirks belong only in
the Node facade.

## Core principles

Model Node compatibility as a small algebra of capabilities, resources, and
protocols—not as a collection of deeply separate Node-module subsystems.

The conceptual flow is:

```text
Node API surface
    ↓
Protocol layer
    ↓
Resource algebra
    ↓
Capability algebra
    ↓
Operating-system and runtime primitives
```

### Capabilities

Expose the smallest native operations through stable interfaces:

```text
read · write · open · close · spawn · wait · sleep · resolve · random
```

Filesystem, networking, HTTP, and subprocess APIs should compose these
primitives instead of creating parallel low-level machinery.

### Resources

Every externally stateful entity is a generic resource identified by a
`ResourceId`: files, sockets, listeners, pipes, processes, timers, TTYs, and
DNS requests. Generic operations apply to resources:

```text
(resource, Read)
(resource, Write)
(resource, Close)
(resource, Poll)
(resource, Control(operation))
```

### Operations

Asynchronous work uses one operation algebra and one completion model:

```text
Op { resource, kind, state, continuation }

Pending → Ready(value)
        → Error(error)
        → Cancelled
```

Timers, filesystem access, sockets, DNS, and subprocesses should share this
model rather than each introducing a bespoke state machine.

### Protocols

Higher-level behavior is composed as transformations over resources and
streams. For example:

```text
TCP → TLS → HTTP → Node http API
File → Readable → Node stream API
```

Streams are protocols over read, write, and backpressure. Event emitters are
observer protocols, promises are completion protocols, and buffers are
byte-view protocols.

### Declarative Node facades

Node modules should be declarative adapters over the lower layers:

```text
node_module! {
    fs {
        "readFile" => async_op(ReadFile),
        "open"     => resource_op(OpenFile),
        "stat"     => op(Stat),
    }
}
```

Declarations should generate argument validation, JavaScript/Rust conversion,
async wrapping, errors, exports, documentation, and ordinary tests. Keep only
irreducible compatibility behavior handwritten.

The strongest compression rule is:

```text
Everything external      = Resource
Everything happening     = Operation
Everything compositional = Protocol
Everything Node-specific = Facade
```

## Runtime independence

`JsRuntime` is the engine-independent interface used by the application.
The sole implementation is `quench-runtime`; there is no alternate runtime
selection.

The Node compatibility layer must depend only on `JsRuntime` and capability
interfaces. It must not make direct engine calls or require a particular
JavaScript engine.

## Data-first implementation

API shape, validation, conversions, errors, calling conventions, exports,
capabilities, and compatibility evidence should be represented as data.
Generate repetitive registrations, wrappers, declarations, types, and tests
from that data. Handwrite only irreducible behavior. Prefer small orthogonal
primitives and composition over duplicated subsystem machinery.

## Boundaries

`quench-runtime` must remain unaware of Node modules and Node compatibility
policy. `quench-node` owns the Node host, module cache, source loading,
resolution integration, Node API registry, and compatibility facade. Filesystem
and package resolution may use `oxc-resolver`; host behavior remains handwritten
in `quench-node`.

## Completion criteria

- The default runtime executes the **complete** Node API test suite and all
  examples. Every test in `tests/node/test/parallel` (and any other Node
  test directory the repository tracks) passes through the default runtime;
  no test is silently skipped, narrowed, or marked expected-to-fail.
- The sole quench-runtime executes the **complete** Node API test suite; no
  alternate runtime path or Node test exemption exists.
- No direct engine calls exist in the Node compatibility layer.
- Node API tests are sourced from the upstream Node.js submodule. Test
  bodies are not rewritten, narrowed, or stubbed to make them pass — the
  NodeFacade must satisfy the upstream contract, not the other way around.
- All Rust files are at most 500 lines, functions are at most 40 lines, and
  complexity is at most 10.
- Formatting, repository checks, and custom Rust linters pass.
- Changes are committed and pushed in verified batches. Each batch lands
  one coherent, fully-green slice of the Node API surface; a partial
  green-up is not the goal's finish line.
