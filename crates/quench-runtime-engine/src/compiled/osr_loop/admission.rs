//! Admission of a loop region: the admission record, the region admission and its block, redefinition and dynamic array checks, and the admit pass.
use super::*;

pub(super) struct Admission<'a> {
    pub(super) constants: &'a [Value],
    /// The registers an allocation defines (`boxed`).
    pub(super) boxed: std::collections::BTreeSet<Register>,
    pub(super) roles: std::collections::BTreeMap<Register, Role>,
    pub(super) captures: Vec<u16>,
    pub(crate) fields: Vec<FieldUse>,
    pub(super) numbers: std::collections::BTreeMap<usize, f64>,
    pub(super) values: std::collections::BTreeMap<usize, Value>,
    /// The arrays registers hold that hold a field's value (`held_registers`), each with
    /// whether the region stores into it.
    pub(super) field_arrays: std::collections::BTreeMap<Register, bool>,
}

impl Admission<'_> {
    /// Notes `register` as an indexed array: one a field read defines, else an entry array.
    pub(super) fn array(&mut self, register: Register, written: bool) -> Option<()> {
        match self.field_arrays.get_mut(&register) {
            Some(stored) => {
                *stored |= written;
                Some(())
            }
            None => self.note(register, Role::Array { elements: Representation::Float64, written }),
        }
    }

    pub(super) fn note(&mut self, register: Register, role: Role) -> Option<()> {
        let joined = match self.roles.get(&register).copied() {
            None => role,
            Some(old) => old.join(role)?,
        };
        self.roles.insert(register, joined);
        Some(())
    }

    /// A Number a register an allocation also defines holds boxed: the lane gives each of
    /// its values its own representation.
    pub(super) fn number(&mut self, register: Register) -> Option<()> {
        if self.boxed.contains(&register) { return self.tagged(register); }
        self.note(register, Role::Scalar(Representation::Float64))
    }

    pub(super) fn tagged(&mut self, register: Register) -> Option<()> {
        self.note(register, Role::Scalar(Representation::Tagged))
    }

    pub(super) fn any_constant(&mut self, index: usize) -> Option<Value> {
        let value = *self.constants.get(index)?;
        self.values.insert(index, value);
        Some(value)
    }

    pub(super) fn constant(&mut self, index: usize) -> Option<f64> {
        let number = self.any_constant(index)?.as_number()?;
        self.numbers.insert(index, number);
        Some(number)
    }

    /// Whether the operand names a Number the region reads: a register, noted as one, or a
    /// Number constant.
    pub(super) fn source(&mut self, operand: Operand) -> Option<()> {
        match operand.kind()? {
            OperandKind::Register => self.number(operand.payload()),
            OperandKind::Constant => self.constant(usize::from(operand.payload())).map(|_| ()),
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    /// The result register of an instruction whose result is only a register write.
    pub(super) fn result(&mut self, instruction: WideInstruction) -> Option<Register> {
        (instruction.result_flags() == 0).then_some(())?;
        let register = instruction.result_register();
        self.number(register)?;
        Some(register)
    }
}

/// The region of the loop from `header` to its backward jump at `backedge`, when every
/// instruction in it is one the tier lowers.
pub(crate) fn region(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[FieldSite],
    live: Option<&std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    header: u32,
    backedge: u32,
    method_calls: MethodCalls,
    native_call: &dyn Fn(usize) -> bool,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    let (start, end) = (header as usize, backedge as usize);
    if start > end || end >= decoded.len() || end - start >= MAX_REGION_INSTRUCTIONS {
        return Err(RegionAnalysisFailure {
            reason: RegionRejection::RegionTooLarge,
            pc: backedge,
        });
    }
    let header_live = live.and_then(|live| live.get(start));
    let admitted = admit_region(
        decoded[start..=end].to_vec(),
        usize::from(function.registers),
        constants,
        sites,
        live.cloned(),
        header_live,
        header,
        &std::collections::BTreeSet::new(),
        method_calls,
        native_call,
    );
    // The BaseJIT tier proves each operand where its row reads it and needs no roles: its plan
    // lowers every instruction, so a loop the role analysis rejects is still its span.
    match admitted {
        Err(_) => LoopRegion::span(decoded[start..=end].to_vec(), constants, live, header)
            .map(|region| LoopRegion { stencil_only: true, ..region })
            .ok_or(RegionAnalysisFailure { reason: RegionRejection::RegionTooLarge, pc: backedge }),
        admitted => admitted,
    }
}

/// The region of `instructions`, at PCs from `header`, of a function with `registers`
/// registers whose liveness by PC is `live` (`header_live` at the header); `objects` are
/// registers read as objects beyond those the instructions show, so tagged
/// (`tagged_registers`) and holding a field's value where they can (`held_registers`).
#[allow(clippy::too_many_arguments)]
pub(super) fn admit_region(
    instructions: Vec<WideInstruction>,
    registers: usize,
    constants: &[Value],
    sites: &[FieldSite],
    live: Option<std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    header_live: Option<RegisterSet>,
    header: u32,
    objects: &std::collections::BTreeSet<Register>,
    method_calls: MethodCalls,
    native_call: &dyn Fn(usize) -> bool,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    // A loop whose every iteration leaves (at a call, a throw) is still a region where the stencil tier runs:
    // BaseJIT lowers each instruction (`base_region::steps`), and Cranelift never compiles the loop (`stencil_only`).
    let leaves = iteration::iterates_natively(&instructions, header, method_calls, native_call).err();
    if let Some(offset) = leaves && !quench_runtime::vm::tier_policy::base_runs() {
        return Err(RegionAnalysisFailure::at(RegionRejection::UnsupportedInstruction(instructions[offset].op() as u16), header, offset));
    }
    let stencil_only = leaves.is_some();
    match admit_register_roles(
        instructions.clone(), registers, constants, sites, live.clone(), header_live, header, objects, method_calls,
    ) {
        Ok(region) => Ok(LoopRegion { stencil_only, ..region }),
        // A loop the register analysis rejects is admitted on value identities where it can be; a
        // call the renamed loop stops at is the inliner's (`is_call_boundary`).
        Err(failure) => match lifetime::admit_lifetime_roles(instructions, registers, constants, sites, live, header, objects, method_calls) {
            Some(Ok(region)) => Ok(LoopRegion { stencil_only, ..region }),
            Some(Err(renamed)) if renamed.is_call_boundary() => Err(renamed),
            _ => Err(failure),
        },
    }
}

/// The region offsets a jump lands on.
pub(super) fn block_starts(instructions: &[WideInstruction], header: u32) -> std::collections::BTreeSet<usize> {
    instructions.iter()
        .filter(|instruction| instruction.op().control_flow_layout().jumps())
        .map(|instruction| (instruction.jump_target() as usize).wrapping_sub(header as usize)).collect()
}

/// Whether an access of `register` is reached by a definition that makes it no array: `None`
/// where the region defines it nowhere, else the offset of such a definition, if any. Only the
/// definitions that reach an access matter: a register a later statement reuses for a number is
/// no array there. An access reached by a definition earlier in its own straight-line code is
/// reached by that one alone; any other access by every definition.
pub(super) fn redefinition(instructions: &[WideInstruction], register: Register,
    block_starts: &std::collections::BTreeSet<usize>) -> Option<Option<usize>> {
    let definitions: Vec<usize> = instructions.iter().enumerate()
        .filter(|(_, instruction)| native_definition(**instruction) == Some(register)).map(|(offset, _)| offset).collect();
    if definitions.is_empty() { return None; }
    let accesses = instructions.iter().enumerate()
        .filter(|(_, instruction)| quench_runtime::staged::indexes_receiver(instruction.op()) && object_reads(**instruction) == Some(register))
        .map(|(offset, _)| offset);
    let mut reaching = std::collections::BTreeSet::new();
    for access in accesses {
        let local = (0..access).rev().find(|at| definitions.contains(at) || block_starts.contains(at))
            .filter(|at| definitions.contains(at));
        match local {
            Some(at) => { reaching.insert(at); }
            None => reaching.extend(definitions.iter().copied()),
        }
    }
    Some(reaching.into_iter().find(|offset| {
        let instruction = instructions[*offset];
        !instruction.op().carries_boxed_value()
    }))
}

/// The registers the region indexes that it defines (other than one holding a field's value at
/// every read, `paths`): they hold no entry array, each access reading the array it holds then
/// (`LoopRegion::dynamic_arrays`). Every definition reaching an access reads an element, field,
/// capture or environment local, or moves one, whose value stays tagged; any other but an exit's
/// is refused.
pub(super) fn dynamic_arrays(instructions: &[WideInstruction], paths: &std::collections::BTreeMap<Register, FieldPath>,
    header: u32) -> Result<std::collections::BTreeSet<Register>, RegionAnalysisFailure> {
    let mut dynamic_arrays = std::collections::BTreeSet::new();
    let block_starts = block_starts(instructions, header);
    for register in instructions
        .iter()
        .filter(|instruction| quench_runtime::staged::indexes_receiver(instruction.op()))
        .filter_map(|instruction| object_reads(*instruction))
        .filter(|register| !paths.contains_key(register))
        .collect::<std::collections::BTreeSet<_>>()
    {
        match redefinition(instructions, register, &block_starts) {
            None => continue,
            Some(Some(offset)) => return Err(RegionAnalysisFailure::at(RegionRejection::ArrayRedefined, header, offset)),
            Some(None) => {}
        }
        dynamic_arrays.insert(register);
    }
    Ok(dynamic_arrays)
}

/// A loaded value's role: boxed where its register is tagged, else the number it defines.
pub(super) fn admit_loaded(admission: &mut Admission<'_>, instruction: WideInstruction,
    tagged: &std::collections::BTreeSet<Register>) -> Option<()> {
    if tagged.contains(&instruction.result_register()) { admission.tagged(instruction.result_register()) }
    else { admission.result(instruction).map(drop) }
}

pub(super) fn admit(
    admission: &mut Admission<'_>,
    instruction: WideInstruction,
    constants: &[Value],
    tagged: &std::collections::BTreeSet<Register>,
    defined_registers: &std::collections::BTreeSet<Register>,
    dynamic_arrays: &std::collections::BTreeSet<Register>,
) -> Option<()> {
    match instruction.op() {
        opcode if quench_runtime::staged::loads_constant(opcode) => {
            let index = instruction.constant_index();
            if tagged.contains(&instruction.result_register()) {
                admission.any_constant(index)?;
                admission.tagged(instruction.result_register())?;
            } else {
                admission.constant(index)?;
                admission.result(instruction)?;
            }
        }
        _ if is_move_row(instruction) => {
            if tagged.contains(&instruction.result_register()) {
                admission.tagged(instruction.result_register())?;
                admission.tagged(instruction.register_b())?;
            } else {
                admission.result(instruction)?;
                admission.number(instruction.register_b())?;
            }
        }
        opcode if quench_runtime::staged::reads_capture(opcode) => {
            (instruction.capture_depth() == 0 && instruction.result_flags() == 0).then_some(())?;
            admit_loaded(admission, instruction, tagged)?;
            let slot = instruction.capture_slot();
            if !admission.captures.contains(&slot) {
                admission.captures.push(slot);
            }
        }
        opcode if quench_runtime::staged::computes_arithmetic(opcode) => {
            if number_comparison(instruction).is_some() {
                (instruction.result_flags() == 0).then_some(())?;
                admission.tagged(instruction.result_register())?;
            } else {
                arithmetic(instruction, instruction.binary_operator())?;
                admission.result(instruction)?;
            }
            admission.source(instruction.operand_b())?;
            admission.source(instruction.operand_c())?;
        }
        opcode if quench_runtime::staged::converts_numeric(opcode) => {
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        opcode if quench_runtime::staged::increments(opcode) => {
            instruction.boolean_flag()?;
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        opcode if quench_runtime::staged::computes_unary(opcode) => {
            if reads_type(instruction) && tagged.contains(&instruction.result_register())
            {
                admission.tagged(instruction.result_register())?;
                admission.tagged(instruction.register_b())?;
            } else {
                unary(instruction, instruction.unary_operator())?;
                admission.result(instruction)?;
                admission.number(instruction.register_b())?;
            }
        }
        // An array the entry guard proved is an object passes.
        opcode if quench_runtime::staged::requires_object_coercible(opcode) => {
            if !dynamic_arrays.contains(&instruction.register_b()) {
                admission.array(instruction.register_b(), false)?;
            }
        }
        opcode if quench_runtime::staged::reads_element(opcode) => {
            admit_loaded(admission, instruction, tagged)?;
            let base = instruction.operand_b().register_index()?;
            // A dynamic array takes no register role (`LoopRegion::dynamic_arrays`).
            if !dynamic_arrays.contains(&base) {
                admission.array(base, false)?;
            }
            admission.source(instruction.operand_c())?;
        }
        // Its field is the region's (`LoopRegion::field_reads`).
        opcode if quench_runtime::staged::reads_named_field(opcode) => {
            admit_loaded(admission, instruction, tagged)?;
        }
        // A tagged value is stored as the lane's element store takes it. A value the loop does not define is an
        // unchanged entry value, which may be any tagged value (`admit_stored`), as a field store's is.
        opcode if quench_runtime::staged::writes_element(opcode) => {
            admit_stored(admission, instruction.register_a(), tagged, defined_registers)?;
            if !dynamic_arrays.contains(&instruction.register_b()) {
                admission.array(instruction.register_b(), true)?;
            }
            admission.number(instruction.register_c())?;
        }
        _ if admitted_store(instruction) => {
            let value_register = instruction.register_a();
            // An unchanged entry value can be any tagged value. The VM logs a register
            // receiver in the generational barrier before native writes begin.
            admit_stored(admission, value_register, tagged, defined_registers)?;
            if let Some(receiver) = instruction.field_store_register() {
                admission.tagged(receiver)?;
            }
        }
        // A capture's or global's store runs as the binding's own (`Lowering::global_cell`), and a binding it does not take
        // leaves; the entry snapshot holds no slot the region stores (`stored_captures`).
        _ if stores_binding(instruction) => admit_binding_store(admission, instruction, tagged, defined_registers)?,
        // A global binding's read (`Vm::global_reads_in_lane`), a Number where arithmetic reads it.
        opcode if quench_runtime::staged::reads_name(opcode) => {
            (instruction.result_flags() == 0).then_some(())?;
            admit_loaded(admission, instruction, tagged)?;
        }
        // A literal or construction allocates without collecting, a due collection leaving
        // first; its arguments and elements pass boxed.
        _ if instruction.op().allocates() => {
            (instruction.result_flags() == 0).then_some(())?;
            admission.tagged(instruction.result_register())?;
        }
        // Its literal's allocation writes it (`Allocation::ArrayOf`).
        opcode if quench_runtime::staged::defines_array_element(opcode) => {}
        opcode if quench_runtime::staged::reads_environment_slot(opcode) => {
            if instruction.result_flags() != 0
                || instruction.returns_from_frame()
                || instruction.numeric_local_target().is_some()
            {
                return None;
            }
            admit_loaded(admission, instruction, tagged)?;
        }
        // The interpreter runs every other instruction at its exact PC, with the whole state
        // written back (binding lookup, calls, allocation, GC, throws).
        _ if exits_before_effects(instruction) => {}
        // A return leaves the loop: the lane resumes at it with the state written back, and
        // runs it.
        opcode if quench_runtime::staged::returns(opcode) || quench_runtime::staged::jumps(opcode) => {}
        _ if comparison_jump(instruction) => {
            if let Some(branch) = identity_branch(instruction, constants) {
                admission.tagged(branch.register)?;
            } else if let Some((register, constant)) = match (
                instruction.operand_b().kind(),
                instruction.operand_c().kind(),
            ) {
                (Some(OperandKind::Register), Some(OperandKind::Constant)) => Some((
                    instruction.operand_b().payload(),
                    usize::from(instruction.operand_c().payload()),
                )),
                (Some(OperandKind::Constant), Some(OperandKind::Register)) => Some((
                    instruction.operand_c().payload(),
                    usize::from(instruction.operand_b().payload()),
                )),
                _ => None,
            }
            .filter(|(register, constant)| {
                quench_runtime::vm::operations::equality_operator(instruction.binary_operator_field())
                    .is_some_and(|(strict, _)| strict)
                    && tagged.contains(register)
                    && constants.get(*constant).and_then(|value| value.as_number()).is_some()
            })
            {
                admission.tagged(register)?;
                admission.constant(constant)?;
            } else if let Some((register, constant)) = strict_constant_test(instruction, constants) {
                admission.tagged(register)?;
                admission.any_constant(constant)?;
            } else if let (Some(left), Some(right)) =
                (instruction.operand_b().register_index(), instruction.operand_c().register_index())
                && quench_runtime::vm::operations::equality_operator(instruction.binary_operator_field()).is_some()
                && [left, right].iter().any(|register| {
                    tagged.contains(register) || matches!(admission.roles.get(register), Some(Role::Scalar(Representation::Tagged)))
                })
            {
                // An (in)equality of tagged values, which boxes a Number operand.
                for register in [left, right] {
                    if !matches!(admission.roles.get(&register), Some(Role::Scalar(Representation::Float64))) {
                        admission.tagged(register)?;
                    }
                }
            } else {
                admission.source(instruction.operand_b())?;
                admission.source(instruction.operand_c())?;
            }
        }
        _ if is_truthiness_row(instruction) => {
            let register = instruction.register_a();
            // A tagged value's truthiness is decided natively where the value alone
            // decides it; any other value exits.
            if tagged.contains(&register) {
                admission.tagged(register)?;
            } else {
                admission.number(register)?;
            }
        }
        _ => return None,
    }
    Some(())
}
