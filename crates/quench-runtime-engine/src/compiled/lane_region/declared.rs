//! TypeScript declared types as u-jit facts. An annotation is a seed, never erased: where a value
//! enters a register the tier checks it once (`conform_declared`) and then trusts the register's
//! narrowed fact, as a declared parameter is checked at entry (the solve's entry cell).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The type a declared language set seeds where a tier checks it exactly: a Number, or a set
/// `scalar::checkable` names (a String, an array or a plain object included). A BigInt, Symbol or a mixed set with Number is not checked here.
pub(crate) fn checkable_declared(bits: u16) -> Option<Type> {
    let ty = Type::languages(bits);
    (super::number_domains::is_double(ty) || scalar::checkable(bits)).then_some(ty)
}

/// The checkable return type of program function `function`, from its declared signature keyed by
/// its source span start (`DeclaredTypes::declared_signature`).
pub(crate) fn declared_return(program: &ResidualProgram, function: u32) -> Option<Type> {
    let start = program.function_get(function as usize)?.source_span?.start;
    let signature = program.declared_types.as_deref()?.declared_signature(start)?;
    checkable_declared(signature.returns)
}

/// The checkable type of program function `function`'s `this` parameter (`DeclaredSignature::this`).
/// An arrow's `this` is its environment's and a derived constructor's is bound by `super()`, so
/// neither declares one here.
pub(crate) fn declared_this(program: &ResidualProgram, function: &quench_runtime::bytecode::Function) -> Option<Type> {
    if function.is_arrow || function.derived_constructor { return None; }
    let start = function.source_span?.start;
    let signature = program.declared_types.as_deref()?.declared_signature(start)?;
    checkable_declared(signature.this)
}

/// The checkable element type of program function `function`'s annotated rest parameter
/// (`DeclaredSignature::rest`). The rest array's elements hold it; the array itself is not typed.
pub(crate) fn declared_rest_element(program: &ResidualProgram, function: &quench_runtime::bytecode::Function) -> Option<Type> {
    if !function.rest { return None; }
    let start = function.source_span?.start;
    let signature = program.declared_types.as_deref()?.declared_signature(start)?;
    checkable_declared(signature.rest)
}

/// The rest parameter's register and its checkable element type, where the function never writes
/// the register (`written`, the unit's written registers): its element reads are then reads of the
/// array the activation made. The rest parameter is the last of the function's `params`.
pub(crate) fn declared_rest_read(program: &ResidualProgram, function: &quench_runtime::bytecode::Function,
    written: &quench_runtime::dataflow::BitSet) -> Option<(Register, Type)> {
    let ty = declared_rest_element(program, function)?;
    let register = scalar::parameter_register(function, usize::from(function.params.checked_sub(1)?))?;
    (!written.contains(usize::from(register))).then_some((register, ty))
}

/// The parameter registers of `function` that the unit never writes (`written`), each with its
/// parameter index, for member reads whose base is a declared parameter (`declared_member_type`).
/// The rest parameter is left out, as its element reads are `declared_rest_read`'s. Only a function
/// with a signature key (its source span start) can declare a parameter; the top level has none.
pub(crate) fn declared_member_reads(program: &ResidualProgram, function: &quench_runtime::bytecode::Function,
    written: &quench_runtime::dataflow::BitSet) -> Vec<(Register, u16)> {
    if program.declared_types.is_none() || function.source_span.is_none() { return Vec::new(); }
    let rest = if function.rest { function.params.checked_sub(1) } else { None };
    (0..function.params).filter(|index| Some(*index) != rest)
        .filter_map(|index| {
            let register = scalar::parameter_register(function, usize::from(index))?;
            (!written.contains(usize::from(register))).then_some((register, index))
        }).collect()
}

/// The checkable type of member `name` of parameter `parameter` of the function keyed by its source
/// span start `function_start`: the member a signature declares on the parameter itself, else the
/// field of the top-level class the parameter is typed by (`DeclaredTypes::declared_parameter_class`).
pub(crate) fn declared_member_type(program: &ResidualProgram, function_start: u32, parameter: u16, name: &str) -> Option<Type> {
    let declared = program.declared_types.as_deref()?;
    let bits = declared.declared_member(function_start, parameter, name)
        .or_else(|| declared.declared_parameter_class(function_start, parameter)
            .and_then(|class| declared.declared_field(class, name)))?;
    checkable_declared(bits)
}

/// The annotated locals of `function` (`Function::local_registers`, named by `local_atoms`) that
/// `DeclaredTypes::declared_local` types checkably, as the register each lives in and its type.
/// Parameters are checked at entry instead (the solve's entry cell). The top level keys
/// as `u32::MAX`, a nested function by its source span start (`compile::typescript::signatures`).
/// A captured local (`Function::local_slot_captured`) is left out.
pub(crate) fn declared_local_types(program: &ResidualProgram, function: &quench_runtime::bytecode::Function) -> Vec<(Register, Type)> {
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
    pub(crate) fn resumes_after(&self, pc: usize) -> bool {
        pc + 1 < self.unit.instructions.len() && !self.consumed.contains(pc + 1)
    }

    /// Conforms a call's declared result where the call completes: the result register holds the
    /// declared type, checked once at the resume point after the call's effect. The guard reads the
    /// result value, so it holds whichever path joined the completion (the direct entry or the VM's call).
    pub(crate) fn conform_declared_result(&mut self, state: &mut State, pc: usize, result: Register, ty: Type) {
        if self.resumes_after(pc) { self.conform_declared(state, pc + 1, result, ty); }
    }

    /// Conforms `register` to the declared `ty` on this path. A fact that already implies `ty` emits
    /// nothing: the register's narrowed fact carries the check, so no later use re-proves it. The
    /// register keeps its representation, as only a join's edge needs the target's view (`conform`).
    fn conform_declared(&mut self, state: &mut State, pc: usize, register: Register, ty: Type) {
        if state.values[usize::from(register)].value.ty.is_subtype(ty) { return; }
        self.conform_register(state, pc, register, ty);
    }

    /// Conforms each annotated local the entry function's own instruction at `pc` defines
    /// (`declared_local_types`) where it is written: the local's annotation is checked once there.
    pub(crate) fn conform_declared_definitions(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) {
        if !self.caller_record(pc) || !self.resumes_after(pc) { return; }
        // The entry function's own `this` read (`LoadThis`) enters its destination with the declared type.
        if let Some(ty) = self.declared_this && matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::LoadThis { .. })) {
            self.conform_declared(state, pc + 1, instruction.result_register(), ty);
        }
        if self.declared_definitions.is_empty() { return; }
        for register in instruction.written_registers() {
            let Some(ty) = self.declared_definitions.iter().find(|(local, _)| *local == register).map(|(_, ty)| *ty) else { continue };
            self.conform_declared(state, pc + 1, register, ty);
        }
    }

    /// Conforms the element an element read of the entry function's rest parameter (`declared_rest`,
    /// the register `base`) yields: the result register holds the declared element type, checked
    /// once where the read completes. Any other base is left alone.
    pub(crate) fn conform_declared_rest_read(&mut self, state: &mut State, pc: usize, base: Register, result: Register) {
        let Some((rest, ty)) = self.declared_rest else { return };
        if base != rest || !self.caller_record(pc) || !self.resumes_after(pc) { return; }
        self.conform_declared(state, pc + 1, result, ty);
    }

    /// Conforms the value a named read `atom` of the entry function's declared parameter (`base`,
    /// one of `declared_members`) yields: the result register holds the member's declared type,
    /// checked once where the read completes. Any other base, or a name the signature does not type, is left alone.
    pub(crate) fn conform_declared_member_read(&mut self, state: &mut State, pc: usize, base: Register, atom: Atom, result: Register) {
        let Some(&(_, parameter)) = self.declared_members.iter().find(|(register, _)| *register == base) else { return };
        let Some(start) = self.declared_member_start else { return };
        if !self.caller_record(pc) || !self.resumes_after(pc) { return; }
        let program = self.program;
        let Some(ty) = declared_member_type(program, start, parameter, &program.atoms[atom as usize]) else { return };
        self.conform_declared(state, pc + 1, result, ty);
        let owner = self.owner_word(state, Some(base));
        narrow_field_read(state, owner, atom, result);
    }
}

/// Narrows the cached read of `atom` on the object `owner` (`base`'s word) that defined `result` to `result`'s conformed fact,
/// the declared type the check just proved of that same value. A later read of the member on this
/// path then takes that type from the cache (`define_field_read`) and emits no second conform. A
/// call clears the cache (`call_effect`), and a join keeps only entries both arrivals hold (`join_states`).
fn narrow_field_read(state: &mut State, owner: Option<ir::Value>, atom: Atom, result: Register) {
    let fact = state.values[usize::from(result)];
    let TaggedView::Value(value) = fact.tagged else { return };
    if let Some(field) = state.proofs.fields.iter_mut()
        .find(|field| Some(field.owner) == owner && field.atom == atom && field.value == value) {
        field.ty = fact.value.ty;
    }
}
