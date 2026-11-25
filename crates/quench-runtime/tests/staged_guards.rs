//! The one-semantics guards, as an integration test so they run against the built layers
//! (`quench-layers`): which opcodes the staged layer decodes and that every tier derives from it
//! (`coverage`), which stage traits each tier implements (`stage_coverage`), that no rule has a
//! second home (`semantics_duplication`), and that every analysis reaches its fixpoint through the one
//! driver, `dataflow::solve` (`fixpoint_drivers`). Each reads source text and a shrink-only debt file
//! in `src/staged/`.
extern crate quench_layers;

#[path = "staged_guards/coverage.rs"]
mod coverage;
#[path = "staged_guards/fixpoint_drivers.rs"]
mod fixpoint_drivers;
#[path = "staged_guards/semantics_duplication.rs"]
mod semantics_duplication;
#[path = "staged_guards/stage_coverage.rs"]
mod stage_coverage;
