//! TypeScript declared types as u-jit facts. An annotation is a seed, never erased: where a value
//! enters a register the tier checks it once (`conform_register`) and then trusts the fact, as a
//! declared parameter is checked at entry (`scalar::declared_entry_types`).

use super::*;

/// The type a declared language set seeds where a tier checks it exactly: a Number, or a set
/// `scalar::checkable` names. A String, BigInt, Symbol or a mixed set with Number is not checked here.
pub(super) fn checkable_declared(bits: u16) -> Option<Type> {
    let ty = Type::languages(bits);
    (ty == Type::NUMBER || scalar::checkable(bits)).then_some(ty)
}

/// The checkable return type of program function `function`, from its declared signature keyed by
/// its source span start (`DeclaredTypes::declared_signature`).
pub(super) fn declared_return(program: &ResidualProgram, function: u32) -> Option<Type> {
    let start = program.function_get(function as usize)?.source_span?.start;
    let signature = program.declared_types.as_deref()?.declared_signature(start)?;
    checkable_declared(signature.returns)
}

/// The annotated locals of `function` (`Function::local_registers`, named by `local_atoms`) that
/// `DeclaredTypes::declared_local` types checkably, as the register each lives in and its type.
/// Parameters are checked at entry instead (`scalar::declared_entry_types`). The top level keys
/// as `u32::MAX`, a nested function by its source span start (`compile::typescript::signatures`).
/// A captured local (`Function::local_slot_captured`) is left out.
pub(super) fn declared_local_types(program: &ResidualProgram, function: &crate::bytecode::Function) -> Vec<(Register, Type)> {
    let Some(declared) = program.declared_types.as_deref() else { return Vec::new() };
    let key = match function.source_span {
        _ if function.parent.is_none() => u32::MAX,
        Some(span) => span.start,
        None => return Vec::new(),
    };
    // A captured local may be written by a closure through its environment, never through this
    // register, so a fact on the register would go stale: only a local this frame alone writes is seeded.
    function.local_registers.iter()
        .filter(|entry| entry.local >= function.params && !function.local_slot_captured(usize::from(entry.local)))
        .filter_map(|entry| {
            let atom = *function.local_atoms.get(usize::from(entry.local))?;
            let ty = checkable_declared(declared.declared_local(key, &program.atoms[atom as usize])?)?;
            Some((entry.register, ty))
        }).collect()
}

impl Lowering<'_, '_> {
    /// Whether a check after the instruction at `pc` can resume at the next PC: a fused successor
    /// (`consumed`) is executed by its predecessor's lowering, so an exit there would run it twice.
    fn resumes_after(&self, pc: usize) -> bool {
        pc + 1 < self.unit.instructions.len() && !self.consumed.contains(pc + 1)
    }

    /// Conforms a call's declared result where the call completes: the result register holds the
    /// declared type, checked once at the resume point after the call's effect. The guard reads the
    /// result value, so it holds whichever path joined the completion (the direct entry or the VM's call).
    pub(super) fn conform_declared_result(&mut self, state: &mut State, pc: usize, result: Register, ty: Type) {
        if self.resumes_after(pc) { self.conform_register(state, pc + 1, result, ty); }
    }

    /// Conforms each annotated local the entry function's own instruction at `pc` defines
    /// (`declared_local_types`) where it is written: the local's annotation is checked once there.
    pub(super) fn conform_declared_definitions(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) {
        if self.declared_definitions.is_empty() || !self.caller_record(pc) || !self.resumes_after(pc) { return; }
        for register in instruction.written_registers() {
            let Some(ty) = self.declared_definitions.iter().find(|(local, _)| *local == register).map(|(_, ty)| *ty) else { continue };
            self.conform_register(state, pc + 1, register, ty);
        }
    }
}
