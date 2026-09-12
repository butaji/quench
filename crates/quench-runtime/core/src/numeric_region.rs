mod analysis;
mod guard;
mod index;
mod model;
mod physical;
mod rewrite;

pub(crate) use analysis::{
    analyze, op_trace_enabled, quote_adjacent_loop, quote_block, stats_enabled, trace,
    trace_enabled,
};
pub(crate) use guard::{GuardFailure, GuardPlan, PropertyGuardFailure, ValidatedRegionContext};
pub(crate) use model::{
    GuardKind, GuardSource, NumericBinary, NumericUnary, QuotedLoop, RegionOp, StaticPropertyAccess,
};
pub(crate) use physical::{
    RegisterConversion, RegisterLocation, RegisterRegionPlan, RegisterRegionReject,
    RegisterRegionStep, Word32Kind, plan_register_region,
};

#[cfg(test)]
mod tests;
