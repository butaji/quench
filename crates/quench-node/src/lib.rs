// `quench-node` is a Node.js-API compatibility host built on top of
// `quench-runtime`. The runtime is a pure JavaScript engine; this
// crate is the only piece of the workspace allowed to know what
// "Node" is. See `docs/adr/0002-quench-node-scope.md` for the
// scope, the data + patterns + machines + effects shape, and the
// v1 module set. The ordered plan is in `docs/NODE-STAGES.md`.
//
// This file is the crate root only. Builtins, state machines,
// codegen templates, and effects land in submodules as the stages
// in `docs/NODE-STAGES.md` come online.
