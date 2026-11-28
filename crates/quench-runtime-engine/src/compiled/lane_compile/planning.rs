use super::*;

/// A lowered lane body: the method it becomes once its code exists.
pub(crate) struct LanePlan {
    pub(crate) unit: lane_unit::Unit,
    pub(crate) lowered: lane_region::LoweredLane,
    pub(crate) fields: Vec<Option<Vec<NativeFieldLoad>>>,
    pub(crate) direct: Box<[DirectArm]>,
    /// Whether every call of it that has run is inlined or direct (`LaneMethod::closed`).
    pub(crate) closed: bool,
    pub(crate) function: u32,
    /// The word its publication sets when one of its Stable facts changes.
    pub(crate) invalidation: quench_runtime::code_dependencies::Invalidation,
}

/// The VM's facts a lane body's lowering may ask (`OnlineFacts`), answered when it is planned for
/// the shapes and naming sites its unit names: the shapes its field loads prove
/// (`Vm::shape_is_stable`), and its `SetFunctionName` sites (`Vm::native_function_name`). A
/// question outside them answers that no fact holds, which folds less and stays exact.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct FactTables {
    pub(super) stable_shapes: std::collections::BTreeSet<u32>,
    pub(super) names: std::collections::BTreeMap<(u16, Atom), (quench_runtime::staged::add_plan::Transition, u64)>,
    /// The capture reads of the unit's own instructions that reach the program root's environment
    /// at a slot the root writes once, by PC, with the function each holds now (`root_constants`).
    pub(super) root_constants: std::collections::BTreeMap<usize, Value>,
    /// The program those root constants belong to: the active one (`Vm::root_slot_written_once`).
    pub(super) root_program: quench_runtime::vm::program_store::ProgramId,
}

/// Whether the `depth`-th enclosing function of `function`, by the program's static parent chain,
/// is the root: depth zero is the parent (a capture read's environment is its closure's scope).
pub(super) fn reaches_root(program: &ResidualProgram, function: u32, depth: u16) -> bool {
    let mut current = function;
    for _ in 0..=depth {
        let Some(parent) = program.function_get(current as usize).and_then(|body| body.parent) else { return false };
        current = parent;
    }
    current == quench_runtime::vm::ROOT_FUNCTION_ID
}

/// The capture reads of `unit`'s own instructions that read a written-once root binding of the
/// active program (`Vm::root_slot_written_once`), by PC, with the value the root environment's
/// slot holds now, where that value is an initialized function. A read reaches the root by
/// `reaches_root`; the root environment is the one its activation promoted (`root_environment`).
pub(super) fn root_constants(vm: &Vm, function: u32, unit: &lane_unit::Unit) -> std::collections::BTreeMap<usize, Value> {
    let program_id = vm.active_program;
    let (Some(program), Some(environment)) = (vm.programs.residual(program_id), vm.programs.root_environment(program_id)) else {
        return Default::default();
    };
    unit.instructions.iter().enumerate().filter_map(|(pc, instruction)| {
        let Some(quench_runtime::staged::Operation::LoadCapture { depth, slot, .. }) = crate::compiled::decoded(instruction) else { return None };
        let own = matches!(unit.origins.get(pc)?, lane_unit::Origin::Caller(_));
        if !own || !reaches_root(program, function, depth) || !vm.root_slot_written_once(program_id, slot) { return None; }
        let value = vm.heap_reads().environment_slot(environment, usize::from(slot))?;
        (value != Value::DELETED && vm.function_record(value).is_some()).then_some((pc, value))
    }).collect()
}

