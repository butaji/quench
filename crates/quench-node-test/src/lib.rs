// `quench-node-test` owns the Node.js test runner: discovery of
// fixtures under `node-tests/` (the pinned upstream submodule),
// composition of Node's test harness exactly as the upstream
// declares it, execution through the host contract, and
// classification of completions into pass / fail / skip / crash.
//
// This crate depends on `quench-node` (the host) and is forbidden
// from modifying the upstream fixture tree or rewriting Node
// harness behavior. See `docs/adr/0002-quench-node-scope.md`.
//
// This file is the crate root only. The runner binary, the
// `triage` diagnostic, and the classifier land in submodules as
// the stages in `docs/NODE-STAGES.md` come online.
