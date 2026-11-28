//! The element view a compiled body's loop arrays take (`lane_region::lower_body`'s `RegionPlan::views`), read from the
//! interpreter's element records: a typed array read in a loop holds its view, not a dense proof that leaves at every entry.
use quench_runtime::vm::Vm;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The view a body's loop arrays take: the typed kind the element records of `function`'s own code were switched to
/// (`quickened_typed_kind`), else dense. A typed array read in a loop then holds its view, not a dense proof that leaves.
pub(crate) fn body_views(vm: &Vm, function: u32, unit: &super::lane_unit::Unit) -> quench_runtime::vm::engine_types::ElementViews {
    let view = vm.programs.views_in_lane(vm.active_program).and_then(|views| views.get(function as usize)).and_then(quench_runtime::vm::program_store::derived_lane_view);
    let sites = unit.instructions.iter().zip(&unit.origins).filter_map(|(instruction, origin)| match (crate::compiled::performed(instruction), origin) {
        (Some(quench_runtime::staged::Operation::ElementRead { .. } | quench_runtime::staged::Operation::ElementWrite { .. }), super::lane_unit::Origin::Caller(pc)) => Some(*pc as usize),
        _ => None,
    });
    view.and_then(|view| quickened_typed_kind(view, sites))
        .map_or(quench_runtime::vm::engine_types::ElementViews::Dense, |kind| quench_runtime::vm::engine_types::ElementViews::Typed(Some(kind)))
}

/// The typed kind the element records of `view` at `pcs` were given at load (`element_typed_row`), where they agree: the
/// view a compiled body's loop arrays take (`lane_region::lower_body`). A record no run reached yet (a loop not entered
/// so far) names no kind. `None` where none is typed, or two kinds are.
fn quickened_typed_kind(view: &crate::dispatch_fast::LaneView, pcs: impl Iterator<Item = usize>) -> Option<quench_runtime::tier_surface::TypedArrayKind> {
    let mut kind = None;
    for pc in pcs {
        // SAFETY: `pc` names an instruction of `view`'s function, whose record it holds; a typed record's immediate is its kind.
        let record = unsafe { &*view.records.base().add(pc) };
        let Some(next) = (crate::dispatch_fast::is_typed_element_row(record.handler, true) || crate::dispatch_fast::is_typed_element_row(record.handler, false)).then(|| quench_runtime::staged::typed_elements::native_kind_at(unsafe { record.operand.imm }).kind) else { continue };
        if kind.is_some_and(|kind| kind != next) { return None; }
        kind = Some(next);
    }
    kind
}

/// An element access whose record in the function's own code was given a typed kind at load (`element_typed_row`): the
/// kind's view is proved for that kind alone (`kind_at`). The access's type is not this record's: the solve proves it
/// (`FunctionFacts::cell_after`), and the kind names no element class the solve keeps.
#[derive(Clone, Copy)]
pub(crate) struct ObservedKind {
    pub(crate) pc: usize,
    pub(crate) kind: quench_runtime::tier_surface::TypedArrayKind,
}

/// The typed kind the element access at `pc` was observed to take (`ObservedKind`), of `kinds` (sorted by PC).
pub(crate) fn kind_at(kinds: &[ObservedKind], pc: usize) -> Option<quench_runtime::tier_surface::TypedArrayKind> {
    kinds.binary_search_by_key(&pc, |read| read.pc).ok().map(|at| kinds[at].kind)
}

/// The element accesses of `unit` whose records in `function`'s own code were switched to a typed kind
/// (`element_typed_row`), each as an access of that kind. Either proves its view for that kind alone (`kind_at`).
/// An access of any other base leaves at the view's proof.
pub(crate) fn typed_reads(vm: &Vm, function: u32, unit: &super::lane_unit::Unit) -> Vec<ObservedKind> {
    let Some(view) = vm.programs.views_in_lane(vm.active_program).and_then(|views| views.get(function as usize))
        .and_then(quench_runtime::vm::program_store::derived_lane_view) else { return Vec::new() };
    let observed: Vec<ObservedKind> = unit.instructions.iter().zip(&unit.origins).enumerate().filter_map(|(at, (instruction, origin))| {
        let super::lane_unit::Origin::Caller(pc) = origin else { return None };
        let reads = match crate::compiled::performed(instruction) {
            Some(quench_runtime::staged::Operation::ElementRead { .. }) => true,
            Some(quench_runtime::staged::Operation::ElementWrite { .. }) => false,
            _ => return None,
        };
        // SAFETY: `pc` names an instruction of `view`'s function, whose record it holds; a typed record's immediate is its kind.
        let record = unsafe { &*view.records.base().add(*pc as usize) };
        crate::dispatch_fast::is_typed_element_row(record.handler, reads).then(|| quench_runtime::staged::typed_elements::native_kind_at(unsafe { record.operand.imm }))
            .map(|native| ObservedKind { pc: at, kind: native.kind })
    }).collect();
    same_array_accesses(unit, observed)
}

/// `observed` and, for each element access no record observed (code the interpreter never ran), the kind of an
/// observed access whose base is the same array: the same register, or one a `Move` copied it to and nothing else
/// writes. The kind is a fact of the array, not of the site: its access proves the view for that kind once (or takes
/// the view an earlier access kept, `Proofs::typed`) and leaves where the base is another value.
fn same_array_accesses(unit: &super::lane_unit::Unit, mut observed: Vec<ObservedKind>) -> Vec<ObservedKind> {
    use quench_runtime::staged::Operation;
    let base_of = |instruction| match crate::compiled::performed(instruction) {
        Some(Operation::ElementRead { base, .. }) => Some((base, true)),
        Some(Operation::ElementWrite { base, .. }) => Some((base, false)),
        _ => None,
    };
    // A register's array: the register a sole `Move` copied into it, followed back (each written once).
    let mut writes = std::collections::HashMap::<quench_runtime::bytecode::Register, usize>::new();
    for instruction in &unit.instructions { for register in instruction.written_registers() { *writes.entry(register).or_default() += 1; } }
    let mut copied = std::collections::HashMap::new();
    for instruction in &unit.instructions {
        if let Some(Operation::Move { destination, source }) = crate::compiled::performed(*instruction) {
            if writes.get(&destination) == Some(&1) { copied.insert(destination, source); }
        }
    }
    let root = |mut register| { for _ in 0..8 { match copied.get(&register) { Some(&source) => register = source, None => break } } register };
    let kinds: Vec<(quench_runtime::bytecode::Register, quench_runtime::tier_surface::TypedArrayKind)> = observed.iter()
        .filter_map(|read| Some((root(base_of(unit.instructions[read.pc])?.0), read.kind))).collect();
    let mut added = Vec::new();
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if observed.iter().any(|read| read.pc == pc) { continue; }
        let Some((base, _)) = base_of(*instruction) else { continue };
        let base = root(base);
        let mut same = kinds.iter().filter(|(known, _)| *known == base).map(|(_, kind)| *kind);
        let Some(kind) = same.next() else { continue };
        if same.any(|other| other != kind) { continue; }
        if quench_runtime::staged::typed_elements::NativeKind::new(kind).is_none() { continue; }
        added.push(ObservedKind { pc, kind });
    }
    observed.extend(added);
    observed.sort_by_key(|read| read.pc);
    observed
}

/// The element type each typed element read `kinds` names yields (`ObservedKind`, its kind's element type), sorted by PC: the
/// flow's seed for the reads the interpreter's records took a kind for (`facts::Seeds::typed_reads`).
pub(crate) fn kind_element_types(kinds: &[ObservedKind], instructions: &[quench_runtime::bytecode::WideInstruction]) -> Vec<super::facts::TypedRead> {
    kinds.iter().filter(|read| matches!(crate::compiled::performed(instructions[read.pc]), Some(quench_runtime::staged::Operation::ElementRead { .. })))
        .filter_map(|read| Some((read.pc, quench_runtime::staged::typed_elements::NativeKind::new(read.kind)?.element_type())))
        .collect()
}
