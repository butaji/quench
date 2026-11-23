//! The one-semantics guards, as an integration test so they run against the built layers
//! (`quench-layers`): which opcodes the staged layer decodes and that every tier derives from it
//! (`coverage`), which stage traits each tier implements (`stage_coverage`), and that no rule has a
//! second home (`semantics_duplication`). Each reads source text and a shrink-only debt file in
//! `src/staged/`.
extern crate quench_layers;

#[path = "staged_guards/coverage.rs"]
mod coverage;
#[path = "staged_guards/semantics_duplication.rs"]
mod semantics_duplication;
#[path = "staged_guards/stage_coverage.rs"]
mod stage_coverage;
