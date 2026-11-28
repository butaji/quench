//! Tagged registers of a loop region: the dataflow that proves which registers must stay tagged, and the boxed definitions it follows.
use super::*;

/// Whether `instruction` is a comparison branch (`JumpBinaryFalse`), by its staged decode.
pub(super) fn comparison_jump(instruction: WideInstruction) -> bool {
    // merge: kept as a helper because several admission arms test it; it now asks the staged query.
    quench_runtime::staged::compare_jump(instruction.op())
}

/// A nullish comparison branch that can be emitted from the tagged value word. Strict
/// identity compares one constant. Loose equality against null or undefined matches either
/// nullish value; Abstract Equality does not coerce an object when the other operand is null.
pub(super) fn identity_branch(instruction: WideInstruction, constants: &[Value]) -> Option<IdentityBranch> {
    if !quench_runtime::staged::compare_jump(instruction.op()) {
        return None;
    }
    quench_runtime::vm::operations::equality_operator(instruction.binary_operator_field())?;
    let (register, constant) = match (instruction.operand_b().kind()?, instruction.operand_c().kind()?) {
        (OperandKind::Register, OperandKind::Constant) => (
            instruction.operand_b().payload(),
            *constants.get(usize::from(instruction.operand_c().payload()))?,
        ),
        (OperandKind::Constant, OperandKind::Register) => (
            instruction.operand_c().payload(),
            *constants.get(usize::from(instruction.operand_b().payload()))?,
        ),
        _ => return None,
    };
    let nullish_constant = constant.is_null() || constant.is_undefined();
    nullish_constant.then_some(IdentityBranch { register })
}

/// The register and constant index of a strict (in)equality jump of a register and a constant that is
/// no Number (`=== "o"`): the value a string the region compares holds by its identity or its units,
/// which its tagged word carries (`admit`, `tagged_registers_with_failure`).
pub(super) fn strict_constant_test(instruction: WideInstruction, constants: &[Value]) -> Option<(Register, usize)> {
    if !comparison_jump(instruction)
        || !quench_runtime::vm::operations::equality_operator(instruction.binary_operator_field()).is_some_and(|(strict, _)| strict)
    {
        return None;
    }
    let (register, index) = match (instruction.operand_b().kind()?, instruction.operand_c().kind()?) {
        (OperandKind::Register, OperandKind::Constant) => (instruction.operand_b().payload(), instruction.operand_c().payload()),
        (OperandKind::Constant, OperandKind::Register) => (instruction.operand_c().payload(), instruction.operand_b().payload()),
        _ => return None,
    };
    let index = usize::from(index);
    constants.get(index)?.as_number().is_none().then_some((register, index))
}

/// Whether `register` has definitions in `instructions` and every one is a `load`, which may
/// give an object, or a move of a register only such loads define (`const p = a[i]; p.x`: the
/// element read's register reaches the receiver through a `Move`) or of an entry value.
pub(super) fn loads_alone_define(instructions: &[WideInstruction], register: Register, load: fn(WideInstruction) -> bool) -> bool {
    loads_define(instructions, register, load, &mut Vec::new())
}

/// `loads_alone_define`, through the moves not yet `followed`.
pub(super) fn loads_define(instructions: &[WideInstruction], register: Register, load: fn(WideInstruction) -> bool, followed: &mut Vec<Register>) -> bool {
    if followed.contains(&register) {
        return false;
    }
    followed.push(register);
    let mut definitions = instructions.iter().filter(|candidate| candidate.defined_register() == Some(register)).peekable();
    definitions.peek().is_some() && definitions.all(|definition| load(*definition)
        || (matches!(crate::compiled::decoded(*definition), Some(quench_runtime::staged::Operation::Move { .. })) && moved_from_load(instructions, definition.register_b(), load, followed)))
}

/// Whether a move's `source` may hold what a load gives: loads alone define it, or the loop
/// defines it not at all, so it holds its entry value (`let p = head; while (p) p = p.next`).
pub(super) fn moved_from_load(instructions: &[WideInstruction], source: Register, load: fn(WideInstruction) -> bool, followed: &mut Vec<Register>) -> bool {
    !instructions.iter().any(|candidate| candidate.defined_register() == Some(source)) || loads_define(instructions, source, load, followed)
}

pub(super) fn field_reads_alone_define(instructions: &[WideInstruction], register: Register) -> bool {
    loads_alone_define(instructions, register, is_field_read)
}

/// A named field read (`GetField`).
pub(super) fn is_field_read(instruction: WideInstruction) -> bool {
    quench_runtime::staged::reads_named_field(instruction.op())
}

/// An element read (`GetIndex`).
// merge: HEAD also matched `ElementReadOperand`; theirs' staged query covers `GetIndex` only.
pub(super) fn is_element_read(instruction: WideInstruction) -> bool {
    quench_runtime::staged::reads_element(instruction.op())
}

/// A field read or an element read: the loads whose result a field read may take as its receiver.
pub(super) fn is_receiver_load(instruction: WideInstruction) -> bool {
    is_field_read(instruction) || is_element_read(instruction)
}

/// Traces a tagged-register rejection at `offset` (`QUENCH_TRACE=osr-reject`).
pub(super) fn trace_tagged_reject(header: u32, offset: usize, result: Register, reason: &str) {
    if quench_runtime::trace::Channel::OsrReject.on() {
        eprintln!("QUENCH_JS_JIT_OSR_TAGGED_REJECT header={header} offset={offset} reason={reason} result={result}");
    }
}

/// Registers whose bytecode definitions must stay tagged because a nullish comparison
/// branch observes them. Propagate that fact along moves and through constants; other
/// definitions need a general value denotation before they can be admitted.
pub(super) fn tagged_registers_with_failure(
    instructions: &[WideInstruction],
    constants: &[Value],
    held: &std::collections::BTreeSet<Register>,
    dynamic_arrays: &std::collections::BTreeSet<Register>,
    header: u32,
) -> Result<std::collections::BTreeSet<Register>, usize> {
    let numeric_uses: std::collections::BTreeSet<Register> = instructions
        .iter()
        .copied()
        .filter(|instruction| !is_move_row(*instruction))
        .flat_map(number_reads)
        .collect();
    let mut tagged: std::collections::BTreeSet<Register> = instructions
        .iter()
        .filter_map(|instruction| identity_branch(*instruction, constants).map(|branch| branch.register))
        .chain(
            instructions
                .iter()
                .filter(|instruction| reads_type(**instruction) || instruction.op().allocates()
                    || number_comparison(**instruction).is_some())
                .map(|instruction| instruction.result_register()),
        )
        // An entry array's register; a dynamic array's (one the region defines) is tagged by
        // its values' webs instead (`representations`), as its register may hold Numbers too.
        .chain(instructions.iter().filter_map(|instruction| array_read(*instruction))
            .filter(|register| !dynamic_arrays.contains(register)))
        .chain(instructions.iter().enumerate().flat_map(|(offset, instruction)| call_window_tagged(instructions, &numeric_uses, offset, instruction)))
        // The register a strict test against a non-Number constant compares is tagged: its value may be a string.
        .chain(instructions.iter().filter_map(|instruction| strict_constant_test(*instruction, constants).map(|(register, _)| register)))
        .chain(instructions.iter().filter_map(|instruction| {
            if !quench_runtime::staged::loads_constant(instruction.op()) {
                return None;
            }
            let (destination, index) = (instruction.result_register(), instruction.constant_index());
            constants
                .get(index)?
                .as_number()
                .is_none()
                .then_some(destination)
        }))
        .chain(held.iter().copied())
        // A call's callee that only field or element reads define holds a function (a native one the
        // region calls in place); a register the loop also computes numbers into stays untagged.
        .chain(instructions.iter().filter(|instruction| quench_runtime::staged::calls_ordinary(instruction.op())).map(|instruction| instruction.register_b())
            .filter(|callee| loads_alone_define(instructions, *callee, is_receiver_load)))
        // A field receiver the region's field or element reads give, or one it never writes (its entry value), holds an object.
        .chain(instructions.iter().filter(|instruction| quench_runtime::staged::reads_named_field(instruction.op())).filter_map(|instruction| {
            let receiver = object_reads(*instruction)?;
            (loads_alone_define(instructions, receiver, is_receiver_load) || !instructions.iter().any(|candidate| candidate.defined_register() == Some(receiver)))
                .then_some(receiver)
        }))
        // A branch on a value only field reads give tests its truthiness tagged:
        // a field may hold an object.
        .chain(instructions.iter().filter(|instruction| is_truthiness_row(**instruction)).filter_map(|instruction| {
            let value = instruction.register_a();
            field_reads_alone_define(instructions, value).then_some(value)
        }))
        // A store whose receiver the region redefines passes its receiver, and a value only
        // field reads give (which may be an object), tagged to the VM's cached store
        // (`LoopRegion::dynamic_field_stores`); a Number result is boxed at the store.
        .chain(instructions.iter().filter_map(|instruction| instruction.field_store_register().filter(|&receiver|
            instructions.iter().any(|candidate| candidate.defined_register() == Some(receiver)))))
        // Any store of a value only field reads give stores it tagged: it may be an object,
        // which the entry's barrier on the receiver covers (`loop_entry_in_lane`).
        .chain(instructions.iter().filter(|instruction| admitted_store(**instruction)).filter_map(|instruction| {
            let value = instruction.register_a();
            field_reads_alone_define(instructions, value).then_some(value)
        }))
        .collect();
    propagate_tagged(instructions, constants, header, &mut tagged)?;
    Ok(tagged)
}

/// A fixpoint of one whole-loop pass: `pass` maps a state to the next, and the states settle once a
/// pass returns the state it was given. The one block flows into itself (`dataflow::solve`).
pub(super) struct Passes<S, F> {
    pub(super) pass: F,
    pub(super) edge: [usize; 1],
    pub(super) initial: S,
}

impl<S: Clone + PartialEq, F: FnMut(&S) -> S> quench_runtime::dataflow::Dataflow for Passes<S, F> {
    type State = S;
    fn blocks(&self) -> usize {
        1
    }
    fn edges(&self, _block: usize) -> &[usize] {
        &self.edge
    }
    fn entry_state(&self, _block: usize) -> Option<S> {
        Some(self.initial.clone())
    }
    /// Takes the newer pass's state, where it differs: each pass only grows or shrinks what it
    /// reads, so the states settle.
    fn join(&self, into: &mut S, incoming: &S) -> bool {
        let changed = into != incoming;
        if changed {
            into.clone_from(incoming);
        }
        changed
    }
    fn transfer(&mut self, _block: usize, state: &S) -> S {
        (self.pass)(state)
    }
}

/// Extends `tagged` along moves and type tests to the fixed point where every number definition
/// of a tagged register is one that may carry a boxed value; the offset of one that cannot.
pub(super) fn propagate_tagged(instructions: &[WideInstruction], constants: &[Value], header: u32, tagged: &mut std::collections::BTreeSet<Register>) -> Result<(), usize> {
    let boxed = boxed(instructions);
    let mut failure = None;
    let mut sweeps = Passes {
        pass: |state: &std::collections::BTreeSet<Register>| {
            let mut next = state.clone();
            for (offset, instruction) in instructions.iter().copied().enumerate().rev() {
                if let Err(at) = tag_definition(instruction, offset, (constants, header, &boxed), &mut next) {
                    failure = Some(at);
                    return state.clone();
                }
            }
            next
        },
        edge: [0],
        initial: tagged.clone(),
    };
    let fixed = quench_runtime::dataflow::solve(&mut sweeps).pop().flatten();
    if let Some(at) = failure {
        return Err(at);
    }
    *tagged = fixed.expect("the one block is seeded");
    Ok(())
}

/// What one number definition adds to `tagged`, or the offset where it cannot be admitted.
pub(super) fn tag_definition(instruction: WideInstruction, offset: usize, (constants, header, boxed): (&[Value], u32, &std::collections::BTreeSet<Register>), tagged: &mut std::collections::BTreeSet<Register>) -> Result<(), usize> {
    let Some(result) = number_definition(instruction) else {
        return Ok(());
    };
    if is_move_row(instruction) {
        let source = instruction.register_b();
        if tagged.contains(&result) || tagged.contains(&source) {
            tagged.insert(result);
            tagged.insert(source);
        }
        return Ok(());
    }
    // A register an allocation also defines is held boxed throughout (`boxed`).
    if !tagged.contains(&result) || boxed.contains(&result) {
        return Ok(());
    }
    match tagged_operand(instruction, constants) {
        Ok(operand) => tagged.extend(operand),
        Err(reason) => {
            trace_tagged_reject(header, offset, result, &reason);
            return Err(offset);
        }
    }
    Ok(())
}

/// The register a tagged number definition `instruction` tags in turn (a type test's operand), or the
/// reason it cannot be admitted: only a definition that may carry a boxed value can be tagged.
pub(super) fn tagged_operand(instruction: WideInstruction, constants: &[Value]) -> Result<Option<Register>, String> {
    let op = instruction.op();
    if quench_runtime::staged::loads_constant(op) {
        return constants.get(instruction.constant_index()).map(|_| None).ok_or_else(|| "missing_constant".to_owned());
    }
    if carries_boxed(instruction) {
        return Ok(None);
    }
    // A type test reads its operand's tag, and the VM's cell of a heap value
    // (the type-name index exits for one instead).
    if quench_runtime::staged::computes_unary(op) && reads_type(instruction) {
        return Ok(Some(instruction.register_b()));
    }
    Err(format!("unsupported_tagged_definition op={op:?}"))
}

/// Whether `instruction` defines a register that may carry an arbitrary boxed value: an entry
/// capture, a cached field, a global binding, an element, a fresh allocation or a comparison.
pub(super) fn carries_boxed(instruction: WideInstruction) -> bool {
    let op = instruction.op();
    op.boxes_result() || op.allocates() || (quench_runtime::staged::computes_binary(op) && number_comparison(instruction).is_some())
}

/// The registers of an ordinary call at `offset` that stay boxed: its callee and the arguments
/// moved into its window from registers no number use reads. A call whose callee a field read
/// defined stays under the existing inliner's representation and ownership rules; dynamic and
/// direct calls leave at this exact PC, so their input window must remain boxed through
/// preceding Moves.
pub(super) fn call_window_tagged(instructions: &[WideInstruction], numeric_uses: &std::collections::BTreeSet<Register>, offset: usize, instruction: &WideInstruction) -> Vec<Register> {
    if !quench_runtime::staged::calls_ordinary(instruction.op()) || callee_read_by_field(instructions, offset, instruction) {
        return Vec::new();
    }
    let base = ImmediateLayout::call_window_base(instruction.imm());
    let count = if quench_runtime::staged::calls_array(instruction.op()) {
        1
    } else {
        usize::from(ImmediateLayout::argument_count(instruction.imm()))
    };
    let mut registers = vec![instruction.register_b()];
    registers.extend((0..count)
        .filter_map(|offset| usize::from(base).checked_add(offset).and_then(|register| u16::try_from(register).ok()))
        .filter(|argument| moved_without_numeric_use(instructions, numeric_uses, offset, *argument)));
    registers
}

/// Whether the call at `offset` reads its callee from a register a field read defined.
pub(super) fn callee_read_by_field(instructions: &[WideInstruction], offset: usize, call: &WideInstruction) -> bool {
    instructions[..offset]
        .iter()
        .rev()
        .find(|candidate| candidate.defined_register() == Some(call.register_b()))
        .is_some_and(|definition| quench_runtime::staged::reads_named_field(definition.op()))
}

/// Whether `argument` of the call at `offset` is the end of a chain of moves none of whose registers a
/// number use reads, or an alias of one a number use reads.
pub(super) fn moved_without_numeric_use(instructions: &[WideInstruction], numeric_uses: &std::collections::BTreeSet<Register>, offset: usize, argument: Register) -> bool {
    let mut value = argument;
    let mut moved_argument = false;
    let mut moved_chain = std::collections::BTreeSet::new();
    loop {
        if !moved_chain.insert(value) {
            break;
        }
        if numeric_uses.contains(&value) {
            return false;
        }
        let numeric_move_alias = instructions.iter().any(|candidate| {
            is_move_row(*candidate)
                && candidate.register_b() == value
                && numeric_uses.contains(&candidate.result_register())
        });
        if numeric_move_alias {
            return false;
        }
        let Some(definition) = instructions[..offset]
            .iter()
            .rev()
            .find(|candidate| candidate.defined_register() == Some(value))
        else {
            break;
        };
        if !is_move_row(*definition) {
            break;
        }
        moved_argument = true;
        value = definition.register_b();
    }
    moved_argument
}
