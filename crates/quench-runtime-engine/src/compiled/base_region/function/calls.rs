//! The call rows of a stitch: a call's site or its chain (`call`), a native Math or global native a row computes
//! (`math_call`, `native_call`), and the definition a native call's callee is read from (`sole_definition`, `radix_is_ten`).

use super::*;

/// The row that completes the activation with the register `register` holds.
pub(super) fn returned(register: Register) -> Decoded {
    Decoded { a: Some(Source::Register(register)), ..Decoded::step(Step::Return, None) }
}

/// A call: the stitch's site where its callee inlines, else the call planner's chain; and where
/// it returns its result (`return f(x)`), the completion with it. A tail jump that reuses the
/// caller's frame for the callee's window is not made: the lane stack holds one activation per
/// native frame, and a leave rebuilds them (`lane_materialize_frames`), which a frame the callee
/// took over would leave inexact.
pub(super) fn call(region: &LoopRegion, pc: u32, sites: &mut Sites, instruction: WideInstruction) -> Vec<What> {
    let (plain, returning) = instruction.split_return();
    let shape = CallShape { returning, ..CallShape::of(pc, plain, CallForm::Plain) };
    let first = match math_call(region, sites, &shape).or_else(|| native_call(region, sites, &shape)) {
        Some(rows) => rows.into_iter().map(What::Row).collect(),
        None => match sites.callees.inlined_call(sites.targets, region, &mut sites.context, &shape) {
            Some(site) => vec![What::Site(site)],
            None => vec![What::Row(Decoded::step(Step::Call(shape), Some(shape.result)))],
        },
    };
    let completion = shape.returning.then(|| What::Row(returned(shape.result)));
    first.into_iter().chain(completion).collect()
}

/// A call of a native Math function a row computes (`Math.sqrt(x)`): its callee
/// register was defined by a field read of a Stable global's member that was that native when the
/// code was built (`GlobalRead::NativeMember`), and the call passes one argument. The rows: the
/// callee's identity guarded (any other callee leaves at the call, for the interpreter to call it),
/// then the staged Math operation of the argument into the result, which leaves where it is no
/// Number. The holder's exact shape is held, so the identity is not recycled while the code runs.
/// The interpreter's call protocol, the native's dispatch and its argument copy fold away: partial
/// evaluation of the call on the callee the site proves.
pub(super) fn math_call(region: &LoopRegion, sites: &Sites, shape: &CallShape) -> Option<Vec<Decoded>> {
    use quench_runtime::vm::global_facts::GlobalRead;
    (shape.form == CallForm::Plain && (1..=2).contains(&shape.argc)).then_some(())?;
    let local = (shape.call_pc - region.header) as usize;
    let defined = (0..local).rev().find(|at| region.instructions()[*at].written_registers().any(|register| register == shape.callee))?;
    let GlobalRead::NativeMember { value, native, holder, cell, .. } = *sites.known.globals?.get(&(region.header as usize + defined))? else { return None };
    let kind = fold::math_kind(native)?;
    let binary = matches!(kind, Kind::Min | Kind::Max | Kind::Imul);
    (u16::from(binary) + 1 == shape.argc as u16).then_some(())?;
    let second = binary.then(|| Source::Register(shape.window + 1));
    let operation = Decoded::number(kind, Some(shape.result), Some(Source::Register(shape.window)), second, None);
    // Where the read is the callee's only definition on every path to the call, the callee is the native while the code runs:
    // the holder's exact shape is held (a change of its values retires the body, and a run inside it learns that from the
    // invalidation word after a call), as `fold::pairs` holds it for a folded call. The identity guard is then none.
    if sole_definition(region, defined, local, shape.callee) {
        return Some(vec![Decoded { holder: Some(holder), cell, ..operation }]);
    }
    let guard = Decoded { hole: value.to_bits(), holder: Some(holder), ..Decoded::heap(Heap::Identity, None, Some(Source::Register(shape.callee)), None) };
    Some(vec![guard, operation])
}

/// Whether the read at the local index `defined` of `region` is the only definition of `callee` that reaches the call at `local`:
/// no instruction between writes `callee`, and no jump lands in the span from the read to the call (a path into the span would
/// skip the read, and a resume stub at a PC in it runs what the interpreter reached only through the read).
pub(super) fn sole_definition(region: &LoopRegion, defined: usize, local: usize, callee: Register) -> bool {
    let code = region.instructions();
    let jumped_into = code.iter().filter(|instruction| instruction.op().control_flow_layout().jumps())
        .filter_map(|instruction| (instruction.jump_target() as usize).checked_sub(region.header as usize))
        .any(|target| (defined + 1..=local).contains(&target));
    let written_between = code[defined + 1..local].iter().any(|instruction| instruction.written_registers().any(|register| register == callee));
    !jumped_into && !written_between
}

/// A call of a native a row computes from its arguments, whose callee the call's definition holds by identity: `parseInt(s)` and
/// `parseInt(s, 10)` (the global name's `GlobalRead::NativeSlot`), and `s.charCodeAt(i)` (the method read's
/// `GlobalRead::NativeMember`, whose receiver is the call's `this`). The identity is guarded as `math_call` guards it; the row
/// leaves at the call, where its operands are not the ones it computes, and the definition's own row (the global read, the method
/// read) stays, so the interpreter's call finds the callee in place.
pub(super) fn native_call(region: &LoopRegion, sites: &Sites, shape: &CallShape) -> Option<Vec<Decoded>> {
    use quench_runtime::vm::global_facts::GlobalRead;
    use quench_runtime::heap::Native;
    (shape.form == CallForm::Plain).then_some(())?;
    let local = (shape.call_pc - region.header) as usize;
    let defined = (0..local).rev().find(|at| region.instructions()[*at].written_registers().any(|register| register == shape.callee))?;
    let (value, native, holder) = match *sites.known.globals?.get(&(region.header as usize + defined))? {
        GlobalRead::NativeSlot { value, native, holder, .. } | GlobalRead::NativeMember { value, native, holder, .. } => (value, native, holder),
        _ => return None,
    };
    let row = match native {
        Native::ParseInt if shape.argc == 1 || (shape.argc == 2 && radix_is_ten(region, sites, local, shape.window + 1)) =>
            Decoded::heap(Heap::ParseInt, Some(shape.result), Some(Source::Register(shape.window)), None),
        Native::StringCharCodeAt if shape.argc == 1 =>
            Decoded::heap(Heap::CharCodeAt, Some(shape.result), Some(Source::Register(shape.this)), Some(Source::Register(shape.window))),
        _ => return None,
    };
    let guard = Decoded { hole: value.to_bits(), holder: Some(holder), ..Decoded::heap(Heap::Identity, None, Some(Source::Register(shape.callee)), None) };
    Some(vec![guard, row])
}

/// Whether `register` holds the Number 10 at the instruction `local` of the region: the last write before it is a `LoadConst`
/// of 10 (`parseInt`'s radix).
pub(super) fn radix_is_ten(region: &LoopRegion, sites: &Sites, local: usize, register: Register) -> bool {
    let Some(at) = (0..local).rev().find(|at| region.instructions()[*at].written_registers().any(|written| written == register)) else { return false };
    let load = region.instructions()[at];
    load.op().class() == Class::LoadConst && sites.known.constants.get(load.constant_index()).and_then(|value| value.as_number()) == Some(10.0)
}
