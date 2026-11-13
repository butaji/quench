//! A call the region runs natively (`osr_probe`'s call arm): the callee's compiled
//! parameters-only body runs in place, so an iteration no longer leaves for the lane at each
//! call. Every register live at the call is spilled first, which roots it for a collection
//! inside the callee.

use super::*;

/// Whether a call runs natively: only where the region pinned nothing at entry (no held
/// field, no entry array, no global capture), so a callee cannot make a pin stale.
pub(super) fn native_calls(
    rooted: &std::collections::BTreeMap<Register, FieldPath>,
    globals: &[(crate::bytecode::Atom, u16)],
    instructions: &[WideInstruction],
    wanted: bool,
) -> bool {
    wanted && rooted.is_empty() && globals.is_empty()
        && !instructions.iter().any(|instruction| instruction.op().is_indexed_access())
}

/// Admits a call: only a plain call with a register window; an `eval` call and an argument
/// array stay exits.
pub(super) fn admits(instruction: WideInstruction) -> Option<()> {
    (instruction.op().immediate_layout() == ImmediateLayout::CallWindowWithEvalFlags).then_some(())?;
    (instruction.result_flags() == 0 && !instruction.direct_eval() && !instruction.parameter_eval())
        .then_some(())?;
    Some(())
}

/// The registers a native call writes as values: its result, the callee's value.
pub(super) fn tagged(instructions: &[WideInstruction]) -> impl Iterator<Item = Register> + '_ {
    instructions.iter()
        .filter(|instruction| instruction.op().class() == Class::Call)
        .map(|instruction| instruction.result_register())
}

/// The registers a call reads: its callee and its argument window.
#[inline]
pub(super) fn reads(instruction: WideInstruction, native_calls: bool) -> Vec<(Register, Type)> {
    if !native_calls || admits(instruction).is_none() {
        return Vec::new();
    }
    let window = instruction.call_window();
    let arguments = (0..window.count).filter_map(|offset| {
        let register = u16::try_from(usize::from(window.base) + usize::from(offset)).ok()?;
        Some((register, Type::ANY))
    });
    std::iter::once((instruction.register_b(), Type::ANY)).chain(arguments).collect()
}

impl Admission<'_> {
    /// Admits a native call: its result and callee are values, the window keeps its own
    /// representations.
    pub(super) fn admit_call(&mut self, instruction: WideInstruction) -> Option<()> {
        admits(instruction)?;
        self.tagged(instruction.result_register())?;
        self.tagged(instruction.register_b())?;
        let window = instruction.call_window();
        for offset in 0..window.count {
            let register = u16::try_from(usize::from(window.base) + usize::from(offset)).ok()?;
            self.tagged(register)?;
        }
        Some(())
    }
}

impl Lowering<'_, '_> {
    /// Lowers a call to the callee's compiled body, leaving for the lane on any miss.
    pub(super) fn native_call(&mut self, pc: u32, instruction: WideInstruction) -> Option<()> {
        if !self.region.native_calls {
            self.leave(super::super::RegionExit::Fallback(pc));
            return Some(());
        }
        let window = instruction.call_window();
        let callee = self.register(instruction.register_b());
        let callee = self.boxed(callee);
        self.spill_live(pc)?;
        let (callout, context, out) = self.callout_operands();
        let packed = super::super::osr_call::OSR_CALL
            | (u64::from(window.base) << u16::BITS)
            | u64::from(window.count);
        let selector = self.body.builder.ins().iconst(types::I64, i64::try_from(packed).ok()?);
        let call = self.body.builder.ins().call_indirect(self.callout_signature, callout, &[context, selector, callee, out]);
        let status = self.body.builder.inst_results(call)[0];
        let miss = self.body.builder.ins().icmp_imm_u(IntCC::NotEqual, status, 0);
        self.guard(pc, miss);
        let bits = self.body.builder.ins().load(types::I64, flags(), out, 0);
        self.define(instruction.result_register(), Typed::checked_load(bits, Representation::Tagged));
        self.fall_through(pc)
    }

    /// Spills every register live at `pc` to the activation file (an inlined callee's to the
    /// entry's scratch registers), which roots them for a collection inside the call.
    fn spill_live(&mut self, pc: u32) -> Option<()> {
        let registers: Vec<Register> = self.region.entry_registers()
            .filter(|register| self.region.is_live(pc, *register))
            .collect();
        let mut live = Vec::with_capacity(registers.len());
        for register in registers {
            let web = self.webs.checked_read(self.region, pc, register)?;
            live.push((register, web));
        }
        self.store_registers(pc, &live);
        Some(())
    }
}

