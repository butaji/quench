//! Inlined callees of a loop region: the inline callee records, the this and callee capture bits, and the setup checks an inlined callee passes.
use super::*;

/// Where an inlined call's callee is read from: a cached field path, a depth-zero capture, or
/// a caller register the loop never writes (`inline_region`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum CalleeSource {
    Path,
    Capture(u16),
    Register(Register),
}

/// The capture slot bit an inlined callee's capture read carries (`Inlining::callee_captures`):
/// no function's own depth-0 slot reaches it, which `inline_region` checks.
pub(crate) const CALLEE_CAPTURE: u16 = 1 << 15;

/// The capture slot that carries the activation's `this` into a region whose field reads of
/// `this` read its current slots; no function's slot reaches it.
pub(crate) const THIS_CAPTURE: u16 = CALLEE_CAPTURE - 1;

/// What `inline_region` needs of a call's callee.
pub(crate) struct InlineCallee<'a> {
    pub(crate) callee: Value,
    pub(crate) function: u32,
    pub(crate) params: u16,
    pub(crate) registers: u16,
    pub(crate) instructions: Vec<WideInstruction>,
    pub(crate) live: &'a quench_runtime::compile::liveness::RegisterMasks,
}

/// A field read whose value the bytecode's rewrite also stores into a `this` field (the
/// `GetSetThis` fusion: `writes_current_this` with a site's sink), as the instructions it
/// fused: the read of the site's path into the result register, then the `SetThisField`.
/// `None` for any other instruction.
pub(super) fn inlined_set_this(instruction: WideInstruction, sites: &[FieldSite]) -> Option<Vec<WideInstruction>> {
    (quench_runtime::staged::reads_named_field(instruction.op()) && instruction.result_flags() == quench_runtime::bytecode::SET_THIS_REGISTER)
        .then_some(())?;
    // A site's base names a register of the callee in the site table, which renaming does not
    // reach: the read runs as one named read per step from the renamed base.
    let FieldLookup::Site(index) = instruction.field_lookup()? else { return None };
    let site = *sites.get(index)?;
    let result = instruction.result_register();
    let mut parts = vec![super::super::meaning_compiled::field_read_record(result, site.base.0, site.first.1, site.first.0)];
    if let Some((atom, cache_site)) = site.second {
        parts.push(super::super::meaning_compiled::field_read_record(result, FieldBase::register(result).0, cache_site, atom));
    }
    let (sink_atom, sink_site) = site.sink?;
    parts.push(super::super::meaning_compiled::this_field_store_record(result, 0, sink_site, sink_atom));
    Some(parts)
}

/// The ops an inlined callee's instructions may be: those `admit` lowers, its `Return`, and
/// none that names its function's own state (captures, frame, calls).
pub(super) fn inlinable(op: Op) -> bool {
    op.traits().has(quench_runtime::bytecode::Traits::INLINABLE)
}

/// The registers an instruction reads as objects: a field read's receiver, an indexed
/// access's array.
pub(super) fn object_reads(instruction: WideInstruction) -> Option<Register> {
    match instruction.op() {
        opcode if quench_runtime::staged::reads_named_field(opcode) => match instruction.field_lookup()? {
            FieldLookup::Atom { base, .. } => base.register_index(),
            _ => None,
        },
        _ if let Some(array) = array_read(instruction) => Some(array),
        _ => instruction.field_store_register(),
    }
}

/// The register an indexed access, or the coercibility check before one, reads as its array.
pub(super) fn array_read(instruction: WideInstruction) -> Option<Register> {
    match instruction.op() {
        opcode if quench_runtime::staged::reads_element(opcode) => instruction.operand_b().register_index(),
        opcode if quench_runtime::staged::indexes_receiver(opcode) => Some(instruction.register_b()),
        _ => None,
    }
}

/// Whether `instruction` names `register` (a read or a write); an instruction `renamed`
/// cannot rename names every register.
pub(super) fn mentions(instruction: WideInstruction, register: Register, sites: &[FieldSite]) -> bool {
    // Only a GetField carries a field lookup immediate, so the op is checked first.
    if quench_runtime::staged::reads_named_field(instruction.op()) && matches!(instruction.field_lookup(), Some(FieldLookup::Site(_))) {
        return instruction.result_register() == register
            || field_base(instruction, sites).is_none_or(|base| base.register_index() == Some(register));
    }
    let named = std::cell::Cell::new(false);
    let rename = |name: Register| {
        named.set(named.get() || name == register);
        name
    };
    renamed(instruction, &rename, register, &|target| Some(target)).is_none() || named.get()
}

/// The registers of an inlining region that hold objects: each call's receiver and, where its callee's read is
/// checked, the callee's register, which holds the function its read gives.
pub(super) fn object_registers(calls: &inlining::PcMap<inlining::Site<'_>>, checked_reads: &inlining::PcMap<u32>, caller: &[WideInstruction], header: u32) -> std::collections::BTreeSet<Register> {
    calls
        .iter()
        .flat_map(|(pc, site)| {
            let callee = checked_reads.contains(pc).then(|| caller[(pc - header) as usize].register_b());
            std::iter::once(site.frame.receiver).chain(callee)
        })
        .collect()
}

/// Whether the setup `run` before the call at `pc` can resume at `resume`: it only computes, no jump
/// lands inside it, and it defines no register live at the resume.
pub(super) fn setup_resumes(run: &[WideInstruction], (resume, pc): (u32, u32), at_resume: RegisterSet, targets: &std::collections::BTreeSet<u32>) -> bool {
    run.iter().all(|instruction| pure_setup(instruction.op()))
        && (resume + 1..=pc).all(|pc| !targets.contains(&pc))
        && run.iter().all(|instruction| {
            instruction
                .defined_register()
                .is_none_or(|register| !quench_runtime::compile::liveness::register_in(at_resume, usize::from(register)))
        })
}

/// The ops a call's argument setup may run between reading its callee and the call: none
/// has an effect a second run could repeat, so leaving among them resumes at the read.
pub(super) fn pure_setup(op: Op) -> bool {
    op.traits().has(quench_runtime::bytecode::Traits::PURE_SETUP)
}
