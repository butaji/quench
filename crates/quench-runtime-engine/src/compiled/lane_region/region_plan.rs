use super::*;

/// What a region specializer is asked to produce: how much of the unit must run, how its
/// runs end, and which element views it may assume. The division and the facts it folds are
/// `VmFacts`, so AOT (`Frozen`) and JIT (`Online`) are two plans over one lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegionPlan {
    pub(crate) coverage: Coverage,
    pub(crate) completion: Completion,
    pub(crate) views: ElementViews,
}

/// The proved loads of each field-cache site, flat and indexed by the site (`site_loads`).
pub(crate) type FieldSites = [Option<Vec<NativeFieldLoad>>];

/// The loads of field-cache `site`, if it has any.
pub(crate) fn site_loads(fields: &FieldSites, site: u16) -> Option<&Vec<NativeFieldLoad>> {
    fields.get(usize::from(site))?.as_ref()
}

/// The flat table holding each `(key, value)` at its key, sized to the largest key.
pub(crate) fn flat_table<T>(entries: impl IntoIterator<Item = (usize, T)>) -> Vec<Option<T>> {
    let entries: Vec<(usize, T)> = entries.into_iter().collect();
    let mut table: Vec<Option<T>> = Vec::new();
    table.resize_with(entries.iter().map(|(key, _)| key + 1).max().unwrap_or(0), || None);
    for (key, value) in entries { table[key] = Some(value); }
    table
}

/// Futamura's first projection for one region: the same `lower` a lane body, a static typed
/// region and a numeric region run, parameterized by its `RegionPlan`. AOT and JIT both call
/// this specializer; only the plan and the `VmFacts` division differ.
pub(crate) struct RegionSpecializer<'p> {
    pub(crate) function_id: u32,
    pub(crate) unit: &'p super::super::lane_unit::Unit,
    pub(crate) helpers: LaneHelpers,
    pub(crate) fields: &'p FieldSites,
    pub(crate) global_reads: &'p quench_runtime::vm::global_facts::GlobalReads,
    pub(crate) facts: VmFacts<'p>,
    pub(crate) generic: &'p [usize],
    pub(crate) direct: &'p DirectSites,
    pub(crate) frame_captures: &'p [(usize, usize)],
    pub(crate) isa: &'p dyn cranelift_codegen::isa::TargetIsa,
    /// The typed array kinds the body's element records took (`body_views::typed_reads`), by unit PC (sorted).
    pub(crate) kinds: &'p [super::super::body_views::ObservedKind],
    pub(crate) plan: RegionPlan,
    /// The gate's proof that this region's compile may run (minted by `specialize`).
    pub(crate) admitted: super::super::admission::CompiledUnit,
}

impl Specializer for RegionSpecializer<'_> {
    type Residual = LoweredRegion;

    fn specialize(self, program: &ResidualProgram) -> Result<Self::Residual, String> {
        lower(program, self.function_id, self.unit, self.helpers, self.fields,
            self.global_reads, self.facts, self.generic, self.direct, self.frame_captures,
            self.isa, self.kinds, self.plan.coverage, self.plan.completion, self.plan.views)
            .ok_or_else(|| format!("function {} does not lower under {:?}", self.function_id, self.plan))
    }
}

thread_local! {
    /// The first reason this thread's current lowering declined, and its PC (`usize::MAX`: none).
    static DECLINED: std::cell::Cell<Option<(&'static str, usize)>> = const { std::cell::Cell::new(None) };
    /// The opcodes this thread's current lowering leaves at because it has no lowering for them.
    static UNLOWERED: std::cell::RefCell<std::collections::BTreeSet<String>> = const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
    /// Whether this thread's current lowering wrote a value whose lane disagrees with the
    /// register's representation (`register_define`): the register's variable is per representation,
    /// so the body is no well-formed lowering and is declined.
    static MISMATCH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Notes that the lowering wrote a value of the wrong representation (`register_define`).
pub(in super::super::super) fn note_mismatch() {
    MISMATCH.with(|mismatch| mismatch.set(true));
}

/// Whether the current lowering wrote a representation mismatch.
pub(super) fn mismatched() -> bool {
    MISMATCH.with(std::cell::Cell::get)
}

/// Notes why the lowering declines, for `QUENCH_TRACE=aot-lanes`; the first reason stands.
pub(super) fn declined(why: &'static str, pc: usize) {
    DECLINED.with(|declined| if declined.get().is_none() { declined.set(Some((why, pc))); });
}

