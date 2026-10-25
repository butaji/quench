use crate::bytecode::{
    ControlFlowLayout, FieldLayout, FieldLookup, FieldSite, Function, ImmediateLayout,
    ImmediateRole, Instr, InstructionField, Op, Operand, Register, Superinstruction, WideInstruction,
};

/// A set of registers, one bit each: the uses, definitions and live registers liveness
/// derives, and the root masks a collection reads.
pub(crate) type RegisterSet = u128;

/// A function's live registers before each instruction (`analyze`), held at the width they
/// need: most functions use fewer than 64 registers, so their masks take half the space
/// (V8-v7 RegExp: 95 KiB of masks at its heap's peak, all within 64 registers).
#[derive(Clone, Debug)]
pub(crate) enum RegisterMasks {
    Narrow(Box<[u64]>),
    Wide(Box<[RegisterSet]>),
}

impl RegisterMasks {
    /// `masks` at the narrowest width that holds every one of them.
    pub(crate) fn new(masks: Vec<RegisterSet>) -> Self {
        let narrow: Option<Box<[u64]>> = masks.iter().map(|&mask| u64::try_from(mask).ok()).collect();
        narrow.map_or_else(|| Self::Wide(masks.into_boxed_slice()), Self::Narrow)
    }

    /// The registers live before instruction `pc`, one past the last included.
    pub(crate) fn get(&self, pc: usize) -> Option<RegisterSet> {
        match self {
            Self::Narrow(masks) => masks.get(pc).map(|&mask| RegisterSet::from(mask)),
            Self::Wide(masks) => masks.get(pc).copied(),
        }
    }

    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Narrow(masks) => masks.len(),
            Self::Wide(masks) => masks.len(),
        }
    }

    /// The bytes the masks occupy.
    #[cfg(feature = "profile-memory")]
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::Narrow(masks) => size_of_val::<[u64]>(masks),
            Self::Wide(masks) => size_of_val::<[RegisterSet]>(masks),
        }
    }

    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn iter(&self) -> impl Iterator<Item = RegisterSet> + '_ {
        (0..self.len()).filter_map(|pc| self.get(pc))
    }
}
/// The registers a `RegisterSet` covers. Liveness, and with it root maps, register promotion
/// and move coalescing, covers only functions whose registers fit.
pub(crate) const ROOT_MAP_REGISTERS: u16 = RegisterSet::BITS as u16;

/// Whether `register` is in `set`; a register past the set's width never is.
#[inline(always)]
pub(crate) fn register_in(set: RegisterSet, register: usize) -> bool {
    register < RegisterSet::BITS as usize && set & (1 << register) != 0
}

/// Whether `set`, a live set, proves `register` dead: it covers the register and leaves it
/// out. A register past the set's width may be live.
#[inline(always)]
pub(crate) fn register_dead(set: RegisterSet, register: usize) -> bool {
    register < RegisterSet::BITS as usize && !register_in(set, register)
}

pub(crate) type MethodSite = (u32, u16, Vec<Register>, Option<(u32, u16)>);

/// The argument registers of each method site, by site index: the compiler's site specs, or
/// a finished program's site table.
pub(crate) trait MethodArguments {
    fn arguments(&self, site: usize) -> RegisterSet;
}

impl MethodArguments for [MethodSite] {
    fn arguments(&self, site: usize) -> RegisterSet {
        self[site].2.iter().copied().fold(0, |mask, register| mask | register_bit(register))
    }
}

impl MethodArguments for super::SiteTable<'_, MethodSite> {
    fn arguments(&self, site: usize) -> RegisterSet {
        self.as_slice().arguments(site)
    }
}

impl MethodArguments for crate::bytecode::ResidualProgram {
    fn arguments(&self, site: usize) -> RegisterSet {
        let site = &self.method_sites[site];
        let start = site.argument_start as usize;
        self.method_arguments[start..start + usize::from(site.argument_count)]
            .iter()
            .copied()
            .fold(0, |mask, register| mask | register_bit(register))
    }
}

/// Records which functions liveness covers and, for each covered one, its `Call`s whose
/// result is dead (`dead_call_results`); the collector derives a covered function's per-pc
/// root masks when it first finds one on the stack (`ResidualProgram::register_root_mask`).
pub(crate) fn mark_register_roots<'a>(
    functions: impl IntoIterator<Item = &'a mut Function>,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) {
    for function in functions {
        function.register_roots = register_roots(function, methods, fields, superinstructions);
    }
}

/// `function`'s liveness coverage and dead call results, as `mark_register_roots` records them.
pub(crate) fn register_roots(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> crate::bytecode::RegisterRoots {
    match transfers(function, methods, fields, superinstructions) {
        Some(transfers) => {
            let live = analyze_transfers(function, &transfers);
            crate::bytecode::RegisterRoots::with_dead_call_results(true, dead_call_results(function, &live))
        }
        None => crate::bytecode::RegisterRoots::new(false),
    }
}

/// The pcs, in order, of `function`'s `Call`s that write their result to a register (no
/// result flags) that is dead after the call, under `live`: a lane call may let the callee
/// skip building a result nothing reads (`ImmediateLayout::LANE_CALL_RESULT_UNUSED_FLAG`).
/// A lane derived each from the per-pc masks on its first run, 3-6% of the instructions of
/// cached npm runs (axios, schema-utils, ajv-formats), which a compile-cache hit now keeps.
fn dead_call_results(function: &Function, live: &[RegisterSet]) -> Box<[u32]> {
    function
        .instructions()
        .enumerate()
        .filter(|(pc, instruction)| {
            instruction.op() == crate::bytecode::Op::Call
                && instruction.result_flags() == 0
                && live.get(pc + 1).is_some_and(|&after| {
                    register_dead(after, usize::from(instruction.a() & crate::bytecode::REGISTER_MASK))
                })
        })
        .map(|(pc, _)| u32::try_from(pc).expect("a function's pcs fit u32"))
        .collect()
}

pub(crate) fn analyze(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Option<Vec<RegisterSet>> {
    let transfers = transfers(function, methods, fields, superinstructions)?;
    Some(analyze_transfers(function, &transfers))
}

/// Each instruction's (uses, definitions) register masks, for a function whose registers fit
/// the masks; liveness iterates only the live sets over this fixed table.
pub(super) fn transfers(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Option<Vec<(RegisterSet, RegisterSet)>> {
    if function.registers > ROOT_MAP_REGISTERS {
        return None;
    }
    Some(
        function
            .instructions()
            .map(|instruction| transfer(instruction, methods, fields, superinstructions))
            .collect(),
    )
}

/// One instruction's (uses, definitions) register masks, the entry `transfers` records for it.
pub(super) fn transfer(
    instruction: WideInstruction,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> (RegisterSet, RegisterSet) {
    (
        uses(instruction, methods, fields, superinstructions),
        definitions(instruction, superinstructions),
    )
}

/// Live registers before each instruction. A reverse pass settles every instruction whose
/// successors all follow it, so only code with a backward successor iterates to a fixed point.
/// Each instruction's successors are enumerated once, into `successors[starts[pc]..starts[pc
/// + 1]]`, which every pass then reads.
pub(super) fn analyze_transfers(function: &Function, transfers: &[(RegisterSet, RegisterSet)]) -> Vec<RegisterSet> {
    let length = function.code.len();
    let mut starts = Vec::with_capacity(length + 1);
    let mut successors = Vec::with_capacity(length);
    let mut has_backward_successor = false;
    for (pc, instruction) in function.instructions().enumerate() {
        starts.push(successors.len());
        function.for_each_successor(pc, instruction, |successor, _| {
            has_backward_successor |= successor <= pc;
            successors.push(successor);
        });
    }
    starts.push(successors.len());
    let mut live = vec![0; length + 1];
    loop {
        let mut changed = false;
        for pc in (0..length).rev() {
            let output = successors[starts[pc]..starts[pc + 1]]
                .iter()
                .fold(0, |output, &successor| {
                    output | live.get(successor).copied().unwrap_or(0)
                });
            let (read, written) = transfers[pc];
            let next = read | (output & !written);
            if live[pc] != next {
                live[pc] = next;
                changed = true;
            }
        }
        if !changed || !has_backward_successor {
            break;
        }
    }
    add_suspended_exception_roots(function, &mut live);
    live
}

pub(super) fn live_out(
    function: &Function,
    live: &[RegisterSet],
    pc: usize,
    instruction: WideInstruction,
) -> RegisterSet {
    let mut output = 0;
    function.for_each_successor(pc, instruction, |successor, _| {
        output |= live.get(successor).copied().unwrap_or(0);
    });
    output
}

/// Whether a frame paused at `op` can resume with an abrupt completion that enters the
/// enclosing handler: a call that throws, or a suspension point (an awaited rejection, a
/// generator's `throw` or `return`).
fn resumes_into_handler(op: Op) -> bool {
    op.control_flow_layout() == ControlFlowLayout::Call || matches!(op, Op::Await | Op::Yield | Op::YieldStar)
}

pub(super) fn add_suspended_exception_roots(function: &Function, live: &mut [RegisterSet]) {
    for (pc, instruction) in function.instructions().enumerate() {
        if !resumes_into_handler(instruction.op()) {
            continue;
        }
        for handler in &function.handlers {
            if pc as u32 >= handler.start && (pc as u32) < handler.end {
                live[pc + 1] |= live[handler.target as usize];
            }
        }
    }
}

pub(super) fn uses(
    instruction: WideInstruction,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> RegisterSet {
    let field_reads = InstructionField::ALL
        .iter()
        .copied()
        .fold(0, |mask, field| {
            mask | field_uses(instruction, field, fields)
        });
    let result = if instruction.op().result_layout().reads_result_register() {
        register_bit(instruction.result_register())
    } else {
        0
    };
    let packed_uses = match instruction.op().immediate_layout() {
        ImmediateLayout::CallWindow
        | ImmediateLayout::CallWindowWithEvalFlags
        | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags => {
            let window = instruction.call_window();
            range(window.base, window.count)
        }
        ImmediateLayout::RegisterPair => {
            let (first, second) = instruction.register_pair();
            register_bit(first) | register_bit(second)
        }
        _ => 0,
    };
    let indexed_uses = match instruction.op().immediate_role() {
        ImmediateRole::MethodSiteIndex => method_arguments(instruction, methods),
        ImmediateRole::SuperinstructionIndex => superinstructions
            [instruction.superinstruction_index()]
        .code
        .iter()
        .fold(0, |mask, nested| {
            mask | nested_member(*nested).map_or(RegisterSet::MAX, |nested| {
                uses(nested, methods, fields, superinstructions)
            })
        }),
        _ => 0,
    };
    field_reads | result | packed_uses | indexed_uses
}

fn field_uses(instruction: WideInstruction, field: InstructionField, fields: &[FieldSite]) -> RegisterSet {
    let layout = instruction.op().field_layout(field);
    match layout {
        FieldLayout::RegisterWindowBase => {
            let window = instruction.register_window();
            range(window.base, window.count)
        }
        layout if layout.reads_register() => register_bit(field_register(instruction, field)),
        layout if layout.is_operand_field() => {
            let input_operand = match field {
                InstructionField::B => instruction.operand_b(),
                InstructionField::C => instruction.operand_c(),
                InstructionField::A => return 0,
            };
            operand(input_operand.0, fields)
        }
        FieldLayout::FieldBase => field_base(instruction, fields),
        FieldLayout::OptionalReadRegister => instruction.optional_register_b().map_or(0, register_bit),
        FieldLayout::ConstructArguments => match instruction.construct_arguments() {
            crate::bytecode::ConstructArguments::Registers(window) => {
                range(window.base, window.count)
            }
            crate::bytecode::ConstructArguments::Array(register) => register_bit(register),
        },
        _ => 0,
    }
}

pub(super) fn definitions(
    instruction: WideInstruction,
    superinstructions: &[Superinstruction],
) -> RegisterSet {
    let result = if instruction.op().has_result_register()
        && !instruction.returns_from_frame()
        && !instruction.writes_numeric_local()
    {
        register_bit(instruction.result_register())
    } else {
        0
    };
    let field_writes = InstructionField::ALL
        .iter()
        .copied()
        .fold(0, |mask, field| {
            mask | field_definitions(instruction, field)
        });
    let packed_definitions = match instruction.op().immediate_layout() {
        ImmediateLayout::RegisterPair => {
            let (first, second) = instruction.register_pair();
            register_bit(first) | register_bit(second)
        }
        _ => 0,
    };
    let indexed_definitions = match instruction.op().immediate_role() {
        ImmediateRole::SuperinstructionIndex => superinstructions
            [instruction.superinstruction_index()]
        .code
        .iter()
        .fold(0, |mask, nested| {
            mask | nested_member(*nested).map_or(0, |nested| definitions(nested, superinstructions))
        }),
        _ => 0,
    };
    result | field_writes | packed_definitions | indexed_definitions
}

fn field_definitions(instruction: WideInstruction, field: InstructionField) -> RegisterSet {
    let layout = instruction.op().field_layout(field);
    match layout {
        layout if layout.writes_register() => register_bit(field_register(instruction, field)),
        FieldLayout::OptionalRegister => instruction.optional_register_b().map_or(0, register_bit),
        FieldLayout::NumericLocalTarget => instruction
            .numeric_local_store_target()
            .map_or(0, |target| register_bit(target.register)),
        _ => 0,
    }
}

fn field_register(instruction: WideInstruction, field: InstructionField) -> Register {
    instruction.field_value(field)
}

fn field_base(instruction: WideInstruction, fields: &[FieldSite]) -> RegisterSet {
    match instruction.field_lookup() {
        None => 0,
        Some(FieldLookup::Site(index)) => fields
            .get(index)
            .and_then(|site| site.base.register_index())
            .map_or(0, register_bit),
        Some(FieldLookup::Atom { base, .. }) => base.register_index().map_or(0, register_bit),
    }
}

fn operand(value: u16, fields: &[FieldSite]) -> RegisterSet {
    let operand = Operand(value);
    if let Some(register) = operand.register_index() {
        return register_bit(register);
    }
    if operand.kind() == Some(crate::bytecode::OperandKind::Field) {
        return fields
            .get(operand.payload() as usize)
            .and_then(|site| site.base.register_index())
            .map_or(0, register_bit);
    }
    0
}

fn method_arguments(instruction: WideInstruction, methods: &(impl MethodArguments + ?Sized)) -> RegisterSet {
    methods.arguments(instruction.method_site_index())
}

/// A superinstruction member in fixed-width form. Members are packed without a wide table, so
/// a wide member has no decoding here: it counts as reading every register and writing none,
/// which only lengthens liveness.
fn nested_member(member: Instr) -> Option<WideInstruction> {
    (!member.is_wide()).then(|| member.as_wide())
}

fn range(base: u16, count: u16) -> RegisterSet {
    (0..count).fold(0, |mask, offset| mask | register_bit(base + offset))
}

/// The set of `register` alone; empty past the set's width.
pub(crate) fn register_bit(register: u16) -> RegisterSet {
    if u32::from(register) < RegisterSet::BITS {
        1 << register
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Op;

    const NO_METHODS: &[MethodSite] = &[];

    #[test]
    fn branch_roots_are_derived_from_successor_uses() {
        let function = Function {
            parent: None,
            name: None,
            inferred_name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_span: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![].into(),
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            instance_initializer: None,
            super_home_atom: None,
            constructible: true,
            class_initializer: false,
            parameter_eval_arguments_error: false,
            unaliased_arguments: false,
            prototype_unobservable: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: false,
            locals: 0,
            local_atoms: vec![].into(),
            environment_atoms: vec![].into(),
            selective_capture_slots: None,
            local_registers: Vec::new().into(),
            inherited_with_scope: false,
            lexical_atoms: vec![].into(),
            global_declarations: None,
            name_bindings: Default::default(),
            binding_sites: vec![].into(),
            source_positions: Default::default(),
            environment_clones: vec![].into(),
            code: vec![
                Instr::new(Op::LoadConst, 0, 0, 0, 0),
                Instr::new(Op::Jump, 0, 0, 0, 3),
                Instr::new(Op::Return, 0, 0, 0, 0),
                Instr::new(Op::Return, 1, 0, 0, 0),
            ],
            wide: vec![],
            registers: 2,
            dispatch: crate::bytecode::DispatchClass::General,
            decoded: Default::default(),
            plain_locals: Default::default(),
            declared_atom_sets: Default::default(),
            activation_bindings: Default::default(),
            live_parameter_reflection: Default::default(),
            static_name_scope: Default::default(),
            dynamic_scope_access: Default::default(),
            js_tier_candidate: Default::default(),
            call_plan: Default::default(),
            initializer: Default::default(),
            handlers: vec![].into(),
            register_roots: Default::default(),
            undefined_register: None,
            parameter_registers: None,
            initial_register: Default::default(),
        };
        let roots = analyze(&function, NO_METHODS, &[], &[]).unwrap();
        assert_eq!(roots[3], register_bit(1));
        assert_eq!(roots[1], register_bit(1));
        assert_eq!(roots[1] & register_bit(0), 0);
    }

    #[test]
    fn operand_windows_root_every_pending_input() {
        for op in [
            Op::WasmStructNew,
            Op::WasmStructNewDesc,
            Op::WasmStructNewDefaultDesc,
            Op::WasmArrayNewFixed,
            Op::WasmArrayNewData,
            Op::WasmArrayNewElem,
            Op::WasmExceptionNew,
            Op::WasmAtomicAccess,
        ] {
            let instruction = Instr::new(op, 0, 1, 3, 0);
            assert_eq!(uses(instruction.as_wide(), NO_METHODS, &[], &[]), register_bit(1) | register_bit(2) | register_bit(3));
            assert_eq!(definitions(instruction.as_wide(), &[]), register_bit(0));
            assert_eq!(uses(Instr::new(op, 0, 0, 0, 0).as_wide(), NO_METHODS, &[], &[]), 0);
        }
    }

    #[test]
    fn optional_registers_read_or_write_by_their_layout() {
        // An appended spread source is read: a collection while it iterates must still see it.
        let append = Instr::new(
            Op::AppendArray,
            1,
            crate::bytecode::encode_optional_register(Some(2)),
            1,
            0,
        );
        assert_eq!(uses(append.as_wide(), NO_METHODS, &[], &[]), register_bit(1) | register_bit(2));
        assert_eq!(definitions(append.as_wide(), &[]), 0);
        // A store's optional register receives a copy of the stored value.
        let store = Instr::new(
            Op::StoreLocalPlain,
            1,
            crate::bytecode::encode_optional_register(Some(3)),
            0,
            0,
        );
        assert_eq!(uses(store.as_wide(), NO_METHODS, &[], &[]), register_bit(1));
        assert_eq!(definitions(store.as_wide(), &[]), register_bit(3));
    }

    #[test]
    fn local_target_does_not_define_a_register() {
        let instruction = Instr::new(
            Op::Binary,
            crate::bytecode::NUMERIC_LOCAL_TARGET | 2,
            0,
            1,
            8,
        );
        assert_eq!(definitions(instruction.as_wide(), &[]), 0);
    }
}
