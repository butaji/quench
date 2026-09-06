# Documentation

- [Repository rules](../AGENTS.md): architecture and compatibility constraints.
- [Current work](../tasks/index.json): stencil infrastructure first, then measurement.
- [Stencil JIT specification](stencil-jit-implementation-spec.md): implementation contract and completion gate.
- [Evidence checklist](architecture-evidence.md): how to verify that gate.
- [Architecture gap map](architecture-gap-map.md): provisional cross-workload research and remaining evidence.
- Architecture decisions: [activations](activation-architecture.md),
  [heap identity](heap-identity-architecture.md), and
  [bounded region composition](region-composition-architecture.md), and
  [observability](observability-architecture.md).
- [Performance lanes](performance-lanes.md) and [benchmark integrity](benchmark-integrity.md): measurement rules.
- [Execution-contract tests](execution-contract-tests.md): proposed JS-fixture DSL for semantic outcomes, bounded work and memory ownership.
- [Micros manual](../quench-bench/micros/README.md): diagnostic corpus and qualification.
- [V8_v7 runner](v8_v7.md): end-to-end measurement and limitations.
- [Legacy curriculum](deegen-micro-curriculum.md): mechanism-oriented probes, not qualification.
- [Runtime map](../CONTEXT.md) and [Wasm rules](spec.md): boundaries and terminology.
- [Test262 stages](STAGES.md) and [Node stages](../STAGES.md): runner-consumed data.

Completed task narratives and superseded plans live in Git history, not the
active queue. The pre-cleanup snapshot is commit
`340fc96e6d39718e7008a53eb125db41f0d60155`; inspect an old file with
`git show 340fc96e6d39718e7008a53eb125db41f0d60155:tasks/039.md`.
Historical scores, test counts and no-go findings are not current build evidence.
