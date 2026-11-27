//! The rows a function's instructions lower to: each instruction's row (`lowered`, `cover`), the expanded
//! compare-and-jump rows (`expanded`), the facts those rows decode by (`decode_facts`), and the caller's rows collected
//! from the planned calls and the rest (`parts`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The method call `instruction` at `pc` as a `Method` chain, its callee read into the scratch register past the
/// function's own (`Sites::scratch`), where the call is a method call of a window of consecutive registers (`call_plan::method_shape`).
fn method_call(sites: &Sites, pc: u32, instruction: WideInstruction, returning: bool) -> Option<CallShape> {
    let scratch = Register::try_from(sites.scratch).ok()?;
    super::super::call_plan::method_shape(sites.known.program, instruction, pc, scratch, returning)
}

/// The row of `instruction` at `pc`, which carries no result flag (`cover` strips the return flag
/// of a returning one, `returning`): a `Return` of a register, a `throw` a handler of the function
/// catches natively (`throws::caught`), a call of a form no stitch plans (`Sites::plans`: the
/// construct and stepped forms, the call planner's chain), else what the loop regions' decode covers
/// (a leave where nothing does).
pub(crate) fn lowered(region: &LoopRegion, pc: u32, sites: &Sites, (instruction, returning): (WideInstruction, bool), (facts, helpers): (Facts, LaneHelpers)) -> Decoded {
    let plain = instruction.result_flags() == 0;
    if instruction.op().raises() && let Some(caught) = throws::caught(sites.function, (pc, sites.registers), instruction) {
        return caught;
    }
    if let Some(form) = CallForm::of(instruction) {
        let shape = CallShape { returning, ..CallShape::of(pc, instruction, form) };
        let shape = method_call(sites, pc, instruction, returning).unwrap_or(shape);
        return Decoded::step(Step::Call(shape), Some(shape.result));
    }
    match () {
        _ if crate::compiled::meaning_compiled::is_return(instruction) && plain && instruction.register_a() < sites.registers => returned(instruction.register_a()),
        _ => Decoded { own_step: own_step(instruction), ..locals::fast_row(sites.function, sites.id, instruction, sites.ending)
            .unwrap_or_else(|| decode(region, (facts, sites.known), pc, instruction, helpers)) },
    }
}

/// Whether the row is a fast arm of its instruction: a fast step, a constant written as it is, the value a Stable
/// fact folded (`Decoded::folded`), or a global lexical binding's word read or written in place.
#[cfg(test)]
pub(crate) fn is_fast(row: &Decoded) -> bool {
    matches!(row.step, Some(Step::Fast(_) | Step::Guard(_))) || matches!(row.a, Some(Source::Value(_))) || row.folded.is_some() || row.lexical.is_some()
}

/// The typed helper that makes `instruction`'s step, where one does (`Decoded::own_step`).
fn own_step(instruction: WideInstruction) -> Option<crate::compiled::LaneHelper> {
    use crate::compiled::LaneHelper;
    let plain = instruction.without_return();
    if plain.op().steps_on_owner() { return Some(LaneHelper::OwnerStep); }
    // A value step over registers and constants alone (`value_steps::activation_value_step` reads no local in a frame).
    let held = |operand: quench_runtime::bytecode::Operand| matches!(operand.kind(), Some(quench_runtime::bytecode::OperandKind::Register | quench_runtime::bytecode::OperandKind::Constant));
    let operands = match plain.op().value_step() {
        Some(quench_runtime::bytecode::ValueStep::Unary) => true,
        Some(_) => held(plain.operand_b()) && held(plain.operand_c()),
        None => false,
    };
    if operands && instruction.result_flags() == 0 { return Some(LaneHelper::ValueStep); }
    // A closure captures its owner's binding environment, a frame's or an activation's (`base_body::closure_step`).
    if instruction.result_flags() == 0 && quench_runtime::staged::makes_closure(plain.op()) { return Some(LaneHelper::ClosureStep); }
    // An array literal's element define (`base_body::define_element_step`).
    if instruction.result_flags() == 0 && matches!(quench_runtime::staged::Operation::<(), ()>::decode_literal(instruction, ()), Some(quench_runtime::staged::Operation::DefineElement { .. })) {
        return Some(LaneHelper::DefineElementStep);
    }
    // An allocation the lane's allocator makes (`base_body::allocation_step`).
    if instruction.result_flags() == 0 && quench_runtime::staged::Allocation::decode(instruction, &[]).is_some() { return Some(LaneHelper::AllocationStep); }
    // Any other step the staged row makes as a call-out (`staged::CallOut`, not a decision a jump reads) is the VM's call-out
    // over the running frame (`base_body::call_out_step`).
    let call = quench_runtime::staged::Operation::<(), ()>::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY).and_then(|row| row.call_out());
    if instruction.result_flags() == 0 && !plain.op().control_flow_layout().jumps()
        && call.is_some_and(|(call, _)| !matches!(call, quench_runtime::staged::CallOut::Decide { .. })) {
        return Some(LaneHelper::CallOutStep);
    }
    (instruction.result_flags() == 0 && quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst)).then_some(LaneHelper::ConstantStep)
}

/// Whether a row may be followed by the completion of the activation with its result: it falls
/// through to what follows it, and writes the register the completion reads.
pub(crate) fn completable(row: &Decoded) -> bool {
    row.dst.is_some() && row.target.is_none() && row.step.is_none_or(|step| matches!(step, Step::Helper | Step::Allocate | Step::Fast(_) | Step::Call(_)))
}

impl Sites<'_> {
    /// Whether the stitch plans `instruction` as a call: a body's chain makes every plain call
    /// `callees::chains` admits (a returning one with its completion); a region makes the plain calls
    /// it can run in place (`calls_in_place`), which complete no activation. The other forms are rows
    /// of their own (`lowered`).
    pub(crate) fn plans(&self, instruction: WideInstruction, helpers: LaneHelpers) -> bool {
        match self.ending {
            Ending::Body => chains(instruction),
            Ending::Region => calls_in_place(instruction, helpers),
        }
    }
}

/// The rows of an instruction a body makes in the scratch registers past the function's own (`scratch`): the comparison and the jump on it
/// of an `instanceof` or `in` jump (`sites::compare_jump`). A region reads its entry record, and leaves it to `cover`.
pub(crate) fn expanded(sites: &Sites, pc: u32, instruction: WideInstruction) -> Option<Vec<What>> {
    (sites.ending == Ending::Body && sites.scratch < quench_runtime::bytecode::REGISTER_MASK).then_some(())?;
    let rows = compare_jump(sites.known, pc, instruction, sites.scratch)?;
    #[cfg(test)]
    note_fast(instruction.op());
    Some(rows.into_iter().map(What::Row).collect())
}

/// The registers the rows name above the function's own `registers`: the scratch the rows of `expanded` write, and inlined windows, whose
/// frame slots the body's recovery room covers (`LaneRecoverySpace::reaching`).
pub(crate) fn scratch(rows: &[Decoded], registers: u16) -> usize {
    let method = rows.iter().filter_map(|row| match row.step {
        Some(Step::Call(shape)) => shape.method.map(|method| method.scratch),
        _ => None,
    });
    let reach = rows.iter().flat_map(Decoded::registers).chain(method).map(|register| usize::from(register) + 1).max().unwrap_or(0);
    reach.saturating_sub(usize::from(registers))
}

/// The facts the rows of the instructions that are no planned call decode by: a body's read the heap
/// by its tables; a region's read its entry record, but for what a call row it keeps may change
/// (`Facts::Entry::calls`: a call stitched in changes nothing), and what its stores stale.
pub(crate) fn decode_facts(region: &LoopRegion, sites: &Sites, calls: &[Option<Vec<What>>]) -> Facts {
    if sites.ending == Ending::Body {
        return Facts::Site;
    }
    let kept = calls.iter().flatten().flatten().any(|what| matches!(what, What::Row(row) if matches!(row.step, Some(Step::Call(_)))));
    let stores = region.instructions().iter().any(|instruction| stores_field(sites.known, *instruction) || stores_name(*instruction));
    Facts::Entry { views: if kept { Views::Frame } else { sites.views }, writes: stores || kept }
}

/// The row of the instruction at `pc` that is no planned call: in a body, what it lowers to
/// (`lowered`), where an instruction that carries the return flag (`return a + b`, `return o.p`)
/// lowers as the one without it, followed by the completion with its result
/// (`Decoded::completes`); in a region what the loop regions' decode covers. An instruction no row
/// takes leaves at its PC as it is, flag and all, for the interpreter.
pub(crate) fn cover(region: &LoopRegion, pc: u32, sites: &Sites, instruction: WideInstruction, (facts, helpers): (Facts, LaneHelpers)) -> What {
    if let Some(folded) = super::declared::typeof_fold(sites.function, instruction) {
        return What::Row(folded);
    }
    if let Some(folded) = super::declared::branch_fold(sites.function, sites.known.constants, pc, instruction) {
        return What::Row(folded);
    }
    if sites.ending == Ending::Region {
        return What::Row(decode(region, (facts, sites.known), pc, instruction, helpers));
    }
    let (plain, returning) = instruction.split_return();
    let row = lowered(region, pc, sites, (plain, returning), (facts, helpers));
    if returning && !completable(&row) {
        return What::Row(Decoded::step(Step::Leave, None));
    }
    #[cfg(test)]
    if is_fast(&row) {
        note_fast(instruction.op());
    }
    What::Row(Decoded { completes: returning, ..row })
}

/// The rows whose int32 speculation failed: those of the caller's instructions at the `widened` PCs,
/// and of the inlined callees' at the `widened` keys (`inlined_exit_key`).
pub(crate) fn widened_rows(origins: &[Origin], frames: &[InlinedCall], widened: &[usize]) -> Vec<usize> {
    let widens = |origin: &Origin| match origin {
        Origin::Caller(pc) => widened.contains(&(*pc as usize)),
        Origin::Inlined { frame, pc } => widened.contains(&inlined_exit_key(frames[*frame].call, *pc)),
    };
    origins.iter().enumerate().filter(|(_, origin)| widens(origin)).map(|(k, _)| k).collect()
}

/// What each row's site saw of its operands: the caller's instructions' (`kinds`, from the region's
/// header), none for the rows of an inlined callee.
pub(crate) fn row_kinds(region: &LoopRegion, origins: &[Origin], kinds: &[OperandKinds]) -> Vec<Option<OperandKinds>> {
    origins.iter().map(|origin| match origin {
        Origin::Caller(pc) => kinds.get((*pc - region.header) as usize).copied(),
        Origin::Inlined { .. } => None,
    }).collect()
}

/// Every caller instruction as its parts: its row, or its call's site, and a returning call's
/// completion. The calls are planned first: which a site takes decides what the rest decode by
/// (`decode_facts`).
pub(crate) fn parts(region: &LoopRegion, sites: &mut Sites, helpers: LaneHelpers) -> Vec<Part> {
    let at = |k: usize| region.header + k as u32;
    let planned: Vec<(usize, WideInstruction)> = region.instructions().iter().copied().enumerate().filter(|(_, instruction)| sites.plans(*instruction, helpers)).collect();
    // A native method's field read folds into its call (`fold`): the read plans no row, the call's rows leave at the read.
    let mut folds = fold::pairs(region, sites.known, |instruction| sites.plans(instruction, helpers));
    let reads: Vec<u32> = folds.iter().map(|pair| pair.read).collect();
    let mut calls: Vec<Option<Vec<What>>> = region.instructions().iter().map(|_| None).collect();
    for (k, instruction) in planned {
        calls[k] = Some(match folds.iter().position(|pair| pair.call == at(k)) {
            Some(index) => {
                let mut pair = folds.swap_remove(index);
                // The Number rows between the read and the call leave at the read, as the call's row does.
                for (j, row) in std::mem::take(&mut pair.arithmetic) {
                    calls[j] = Some(vec![What::Resume(row, pair.read)]);
                }
                pair.whats()
            }
            None => call(region, at(k), sites, instruction),
        });
    }
    for (k, instruction) in region.instructions().iter().enumerate() {
        if calls[k].is_none() && !reads.contains(&at(k)) {
            calls[k] = calls::method_call_inlined(region, at(k), sites, *instruction);
        }
    }
    let facts = decode_facts(region, sites, &calls);
    let parts: Vec<Part> = region.instructions().iter().enumerate().flat_map(|(k, instruction)| {
        let pc = at(k);
        let whats = calls[k].take().or_else(|| reads.contains(&pc).then(|| vec![What::Folded])).or_else(|| expanded(sites, pc, *instruction)).unwrap_or_else(|| vec![cover(region, pc, sites, *instruction, (facts, helpers))]);
        whats.into_iter().map(move |what| Part { pc, what })
    }).collect();
    parts
}
