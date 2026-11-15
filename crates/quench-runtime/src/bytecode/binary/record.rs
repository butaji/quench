//! A function's record: its fields in the order a residual program writes them. The writer
//! and the reader walk the same groups, each a run of fields written together.

use super::super::{Atom, EnvironmentClones, GlobalDeclarations, NameBindings, Register, RegisterRoots};
use super::*;
mod write;
pub(super) use write::write_function;
use crate::heap::ThinBoxSlice;
use std::cell::OnceCell;

/// A record's head: its scope, name and parameter shape.
struct RecordHead {
    parent: Option<u32>,
    name: Option<Atom>,
    inferred_name: Option<Atom>,
    is_arrow: bool,
    self_binding_slot: Option<u16>,
    source_span: Option<super::super::SourceSpan>,
    params: u16,
    length: u16,
    parameter_end_pc: u32,
    parameter_atoms: Vec<Atom>,
}

/// A record's kind flags and the class and argument slots that go with them.
struct RecordFlags {
    rest: bool,
    is_async: bool,
    is_generator: bool,
    is_class_constructor: bool,
    derived_constructor: bool,
    instance_initializer: Option<u32>,
    super_home_atom: Option<Atom>,
    constructible: bool,
    class_initializer: bool,
    prototype_unobservable: bool,
    parameter_eval_arguments_error: bool,
    unaliased_arguments: bool,
    simple_parameters: bool,
    strict: bool,
    arguments_slot: Option<u16>,
}

/// A record's local and environment scope.
struct RecordScope {
    locals: u16,
    local_atoms: Vec<Atom>,
    local_registers: Vec<LocalRegister>,
    environment_atoms: Vec<Atom>,
    name_bindings: NameBindings,
    environment_clones: EnvironmentClones,
    selective_capture_slots: Option<Vec<u16>>,
    inherited_with_scope: bool,
    lexical_atoms: Vec<Atom>,
    globals: GlobalDeclarations,
    binding_sites: Vec<BindingSite>,
}

/// A record's register layout: its register count, dispatch class and roots.
struct RecordRegisters {
    registers: u16,
    dispatch: DispatchClass,
    register_roots: RegisterRoots,
    undefined_register: Option<Register>,
    parameter_registers: Option<u16>,
    initial_register: InitialRegister,
}

/// A record's code and its wide instructions.
struct RecordCode {
    code: Vec<Instr>,
    wide: Vec<WideInstruction>,
}

/// A record's handlers, source positions and plain local bits, with its derived facts.
struct RecordTail {
    handlers: Vec<Handler>,
    source_positions: SourcePositions,
    plain_locals: OnceCell<ThinBoxSlice<u64>>,
    derived_facts: u32,
}

/// Reads a function record written by `write_function`.
pub(super) fn read_function(input: &mut BinaryReader<'_>) -> Result<Function, String> {
    let head = read_head(input)?;
    let flags = read_flags(input)?;
    let scope = read_scope(input)?;
    let registers = read_registers(input)?;
    let code = read_code(input, scope.locals)?;
    let tail = read_tail(input, scope.locals)?;
    let packed = input.packed;
    let derived_facts = tail.derived_facts;
    let function = assemble_function(Record { head, flags, scope, registers, code, tail });
    if !packed {
        function.seed_derived_facts(derived_facts);
    }
    Ok(function)
}

/// A record's groups, read in order.
struct Record {
    head: RecordHead,
    flags: RecordFlags,
    scope: RecordScope,
    registers: RecordRegisters,
    code: RecordCode,
    tail: RecordTail,
}

/// The function a record's groups describe.
fn assemble_function(record: Record) -> Function {
    let Record { head, flags, scope, registers, code, tail } = record;
    Function {
        parent: head.parent, name: head.name, inferred_name: head.inferred_name, is_arrow: head.is_arrow,
        self_binding_slot: head.self_binding_slot, source_span: head.source_span, params: head.params,
        length: head.length, parameter_end_pc: head.parameter_end_pc, parameter_atoms: head.parameter_atoms.into(),
        rest: flags.rest, is_async: flags.is_async, is_generator: flags.is_generator,
        is_class_constructor: flags.is_class_constructor, derived_constructor: flags.derived_constructor,
        instance_initializer: flags.instance_initializer, super_home_atom: flags.super_home_atom,
        constructible: flags.constructible, class_initializer: flags.class_initializer,
        prototype_unobservable: flags.prototype_unobservable,
        parameter_eval_arguments_error: flags.parameter_eval_arguments_error,
        unaliased_arguments: flags.unaliased_arguments, simple_parameters: flags.simple_parameters,
        strict: flags.strict, arguments_slot: flags.arguments_slot,
        locals: scope.locals, local_atoms: scope.local_atoms.into(), local_registers: scope.local_registers.into(),
        environment_atoms: scope.environment_atoms.into(),
        selective_capture_slots: scope.selective_capture_slots.map(Into::into),
        inherited_with_scope: scope.inherited_with_scope, environment_clones: scope.environment_clones,
        name_bindings: scope.name_bindings, lexical_atoms: scope.lexical_atoms.into(),
        global_declarations: scope.globals.boxed(), binding_sites: scope.binding_sites.into(),
        source_positions: tail.source_positions, code: code.code, wide: code.wide,
        registers: registers.registers, dispatch: registers.dispatch, decoded: Default::default(),
        plain_locals: tail.plain_locals, declared_atom_sets: Default::default(),
        activation_bindings: Default::default(), live_parameter_reflection: Default::default(),
        static_name_scope: Default::default(), dynamic_scope_access: Default::default(),
        js_tier_candidate: Default::default(), call_plan: Default::default(), initializer: Default::default(),
        handlers: tail.handlers.into(), register_roots: registers.register_roots,
        undefined_register: registers.undefined_register, parameter_registers: registers.parameter_registers,
        initial_register: registers.initial_register,
    }
}

fn read_head(input: &mut BinaryReader<'_>) -> Result<RecordHead, String> {
    let parent = input.option_u32()?;
    let name = input.option_atom()?;
    let inferred_name = input.option_atom()?;
    let is_arrow = input.checked_flag("invalid arrow function flag")?;
    let self_binding_slot = match input.u16()? {
        u16::MAX => None,
        slot => Some(slot),
    };
    let source_span = match input.u8()? {
        0 => None,
        _ => Some(super::super::SourceSpan { start: input.u32()?, end: input.u32()? }),
    };
    Ok(RecordHead {
        parent,
        name,
        inferred_name,
        is_arrow,
        self_binding_slot,
        source_span,
        params: input.u16()?,
        length: input.u16()?,
        parameter_end_pc: input.u32()?,
        parameter_atoms: input.list(|input| input.atom())?,
    })
}

fn read_flags(input: &mut BinaryReader<'_>) -> Result<RecordFlags, String> {
    Ok(RecordFlags {
        rest: input.checked_flag("invalid residual rest flag")?,
        is_async: input.checked_flag("invalid async function flag")?,
        is_generator: input.checked_flag("invalid generator function flag")?,
        is_class_constructor: input.checked_flag("invalid class constructor flag")?,
        derived_constructor: input.checked_flag("invalid derived constructor flag")?,
        instance_initializer: input.option_u32()?,
        super_home_atom: input.option_atom()?,
        constructible: input.checked_flag("invalid constructible function flag")?,
        class_initializer: input.checked_flag("invalid class field initializer flag")?,
        prototype_unobservable: input.checked_flag("invalid prototype observability flag")?,
        parameter_eval_arguments_error: input.checked_flag("invalid parameter eval flag")?,
        unaliased_arguments: input.checked_flag("invalid unaliased arguments flag")?,
        simple_parameters: input.checked_flag("invalid simple parameters flag")?,
        strict: input.checked_flag("invalid strict function flag")?,
        arguments_slot: match input.u16()? {
            u16::MAX => None,
            slot => Some(slot),
        },
    })
}

fn read_local_register(input: &mut BinaryReader<'_>) -> Result<LocalRegister, String> {
    Ok(LocalRegister { local: input.u16()?, register: input.u16()? })
}

fn read_scope(input: &mut BinaryReader<'_>) -> Result<RecordScope, String> {
    let locals = input.u16()?;
    let local_atoms = input.list(|input| input.atom())?;
    let local_registers = input.list(read_local_register)?;
    let environment_atoms = input.list(|input| input.atom())?;
    let name_bindings = read_name_bindings(input)?;
    let environment_clones = EnvironmentClones::read(input)?;
    let selective_capture_slots = match input.u8()? {
        0 => None,
        1 => Some(input.u16s()?),
        _ => return Err("invalid selective capture layout".into()),
    };
    let inherited_with_scope = input.checked_flag("invalid inherited with-scope flag")?;
    let lexical_atoms = input.list(|input| input.atom())?;
    let globals = GlobalDeclarations {
        lexical: input.list(|input| input.atom())?,
        var: input.list(|input| input.atom())?,
        function: input.list(|input| input.atom())?,
        annex_b_var: input.list(|input| input.atom())?,
        immutable: input.list(|input| input.atom())?,
    };
    let binding_sites = input.list(read_binding_site)?;
    Ok(RecordScope {
        locals,
        local_atoms,
        local_registers,
        environment_atoms,
        name_bindings,
        environment_clones,
        selective_capture_slots,
        inherited_with_scope,
        lexical_atoms,
        globals,
        binding_sites,
    })
}

/// A function's name bindings, in the order of their atoms (`restore_atom_order`).
fn read_name_bindings(input: &mut BinaryReader<'_>) -> Result<NameBindings, String> {
    let mut name_bindings = input.list(read_eval_binding)?;
    input.restore_atom_order(&mut name_bindings);
    Ok(NameBindings::flat(name_bindings))
}

fn read_binding_site(input: &mut BinaryReader<'_>) -> Result<BindingSite, String> {
    let resume_pc = input.u32()?;
    let mut bindings = input.list(read_eval_binding)?;
    input.restore_atom_order(&mut bindings);
    Ok(BindingSite { resume_pc, bindings })
}

fn read_registers(input: &mut BinaryReader<'_>) -> Result<RecordRegisters, String> {
    let registers = input.u16()?;
    let dispatch = input.dispatch_class()?;
    let register_roots = read_register_roots(input)?;
    let undefined_register = Some(input.u16()?).filter(|register| *register != NO_UNDEFINED_REGISTER);
    let parameter_registers = Some(input.u16()?).filter(|base| *base != NO_PARAMETER_REGISTERS);
    let initial_register = match input.u8()? {
        0 => InitialRegister::Undefined,
        1 => InitialRegister::I32Zero,
        _ => return Err("invalid residual initial register".into()),
    };
    Ok(RecordRegisters { registers, dispatch, register_roots, undefined_register, parameter_registers, initial_register })
}

/// The register roots of a record: whether they cover the frame, the entry liveness and the
/// dead call results, which must be sorted and only name covered records.
fn read_register_roots(input: &mut BinaryReader<'_>) -> Result<RegisterRoots, String> {
    let covered = input.flag("register root coverage")?;
    let entry_live = input.u16()?;
    let dead_call_results = input.list(BinaryReader::u32)?;
    if !dead_call_results.is_sorted_by(|before, after| before < after) || !covered && !dead_call_results.is_empty() {
        return Err("invalid residual dead call results".into());
    }
    if !covered && entry_live != RegisterRoots::ALL_LIVE_AT_ENTRY {
        return Err("invalid residual entry liveness".into());
    }
    Ok(RegisterRoots::with_entry_live(covered, entry_live, dead_call_results.into()))
}

fn read_code(input: &mut BinaryReader<'_>, locals: u16) -> Result<RecordCode, String> {
    // A narrow instruction naming an atom the reader's id for which no longer fits it is
    // widened, as the compiler widens what does not fit (`Compiler::emit`).
    let mut widened = Vec::new();
    let mut pc = 0;
    let mut code = input.list(|input| {
        let at = pc;
        pc += 1;
        read_code_slot(input, at, &mut widened)
    })?;
    let mut wide = input.list(read_wide_slot)?;
    for (at, instruction) in widened {
        code[at] = Instr::wide(wide.len()).ok_or_else(|| String::from("residual wide index exceeds domain"))?;
        wide.push(instruction);
    }
    if !super::super::local_loads_in_bounds(&code, &wide, locals) {
        return Err("invalid residual local load".into());
    }
    Ok(RecordCode { code, wide })
}

/// One narrow instruction at `at`; one that does not fit is pushed onto `widened`, and its place
/// holds a no-op until the wide entry replaces it.
fn read_code_slot(input: &mut BinaryReader<'_>, at: usize, widened: &mut Vec<(usize, WideInstruction)>) -> Result<Instr, String> {
    let (op, a, b, c, imm) = input.instruction()?;
    if op.is_wide_marker() {
        return Instr::wide_from_fields(a, b, c, imm).ok_or_else(|| String::from("residual wide index exceeds domain"));
    }
    Ok(Instr::try_new(op, a, b, c, imm).unwrap_or_else(|| {
        widened.push((at, WideInstruction::new(op, a, b, c, imm)));
        // Replaced by the widened entry's marker in `read_code`.
        Instr::new(Op::Nop, 0, 0, 0, 0)
    }))
}

fn read_wide_slot(input: &mut BinaryReader<'_>) -> Result<WideInstruction, String> {
    let (op, a, b, c, imm) = input.instruction()?;
    if op.is_wide_marker() {
        return Err("invalid residual wide opcode".into());
    }
    Ok(WideInstruction::new(op, a, b, c, imm))
}

fn read_tail(input: &mut BinaryReader<'_>, locals: u16) -> Result<RecordTail, String> {
    let handlers = input.list(read_handler)?;
    let source_positions = input.source_positions()?;
    let packed = input.packed;
    let plain_bytes = if packed { Vec::new() } else { (0..usize::from(locals).div_ceil(u8::BITS as usize)).map(|_| input.u8()).collect::<Result<Vec<_>, _>>()? };
    let plain_locals: ThinBoxSlice<u64> = if packed { Default::default() } else { crate::bitset::words_from_le_bytes(usize::from(locals), &plain_bytes).into() };
    let derived_facts = if packed { 0 } else { input.u32()? };
    let plain_locals = if packed { OnceCell::new() } else { OnceCell::from(plain_locals) };
    Ok(RecordTail { handlers, source_positions, plain_locals, derived_facts })
}

fn read_handler(input: &mut BinaryReader<'_>) -> Result<Handler, String> {
    let start = input.u32()?;
    let end = input.u32()?;
    let target = input.u32()?;
    let kind = input.u8()?;
    let slot = input.u16()?;
    let slot = match kind {
        THROWN_VALUE_DISCARDED => None,
        THROWN_VALUE_IN_LOCAL => Some(super::super::ThrownValueSlot::Local(slot)),
        THROWN_VALUE_IN_REGISTER => Some(super::super::ThrownValueSlot::Register(slot)),
        _ => return Err("invalid residual handler slot kind".into()),
    };
    let return_target = input.u32()?;
    let return_slot = input.u16()?;
    let with_depth = input.u16()?;
    Ok(Handler {
        start,
        end,
        target,
        slot,
        return_target: (return_target != u32::MAX).then_some(return_target),
        return_slot: (return_slot != u16::MAX).then_some(return_slot),
        with_depth,
    })
}
