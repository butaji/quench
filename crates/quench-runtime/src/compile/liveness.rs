use crate::bytecode::{
    FieldLayout, FieldLookup, FieldSite, Function, ImmediateLayout,
    ImmediateRole, InstructionField, Op, Operand, Register, Superinstruction, WideInstruction,
};

/// A set of registers, one bit each: the uses, definitions and live registers liveness
/// derives, and the root masks a collection reads.
pub use set::RegisterSet; mod set;

mod solve; mod masks; pub(crate) mod windows; pub(crate) mod budget; pub(crate) mod buffers;
// The interpreter runs raw webs once their opcode forms land (`raw-registers-design.md`).
pub(crate) mod raw_webs;
pub use solve::{add_suspended_exception_roots, analyze_graph, analyze_transfers, analyze_wide_with};
pub use masks::RegisterMasks;
use solve::LiveMask;

/// The registers a `RegisterSet` covers. Liveness, and with it root maps, register promotion
/// and move coalescing, covers only functions whose registers fit.
pub const ROOT_MAP_REGISTERS: u16 = RegisterSet::BITS as u16;

/// Whether `register` is in `set`; a register past the set's width never is.
#[inline(always)]
pub fn register_in(set: RegisterSet, register: usize) -> bool {
    register < RegisterSet::BITS as usize && set.contains(register)
}

/// Whether `set`, a live set, proves `register` dead: it covers the register and leaves it
/// out. A register past the set's width may be live.
#[inline(always)]
pub fn register_dead(set: RegisterSet, register: usize) -> bool {
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
        self[site].2.iter().copied().fold(RegisterSet::default(), |mask, register| mask | register_bit(register))
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
            .fold(RegisterSet::default(), |mask, register| mask | register_bit(register))
    }
}

/// Records which functions liveness covers and each covered one's entry bound, each solved once
/// from its own code (a test helper: a lowered function records its roots where it is lowered).
#[cfg(test)]
pub(crate) fn mark_register_roots<'a>(
    functions: impl IntoIterator<Item = &'a mut Function>,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) {
    for function in functions {
        let solved = analyze(function, methods, fields, superinstructions);
        function.register_roots = register_roots_from(function, methods, fields, superinstructions, solved);
    }
}

/// `function`'s register roots, given the live sets `solved` that the lowering solved for `function`
/// as it stands (`analyze`, `None` where it was not solved). The entry bound and the masks are both
/// built from `solved`, so the function is solved once; the masks are kept for the first mask ask
/// where a collection would keep them (`RegisterRoots::keep_solved`). Otherwise the bound and the
/// masks are derived on first ask from the program's tables (`ResidualProgram::entry_fill`,
/// `ResidualProgram::shared_register_root_masks`), so a function nothing activates or collects is
/// never analysed.
pub(crate) fn register_roots_from(function: &Function, methods: &(impl MethodArguments + ?Sized), fields: &[FieldSite], superinstructions: &[Superinstruction], solved: Option<Vec<RegisterSet>>) -> crate::bytecode::RegisterRoots {
    // `RegisterMasks::of` declines over the budget and solves windows past the root map's width.
    let Some(live) = solved.filter(|_| budget::admits(function) && function.registers <= ROOT_MAP_REGISTERS) else {
        return crate::bytecode::RegisterRoots::new(true);
    };
    debug_assert!(analyze(function, methods, fields, superinstructions).as_ref() == Some(&live), "the solved live sets match the code");
    let masks = RegisterMasks::new(live);
    let entry = entry_live(function, &masks);
    crate::bytecode::RegisterRoots::with_solved(entry, (!RegisterMasks::too_large_to_keep(function)).then_some(masks))
}

/// The entry bound of a function whose bound is derived on its first activation: the masks it
/// solves are kept for the first mask ask where a collection would keep them, so that ask does
/// not solve the function again.
pub(crate) fn derive_entry_live_keeping(function: &Function, methods: &(impl MethodArguments + ?Sized), fields: &[FieldSite], superinstructions: &[Superinstruction]) -> u16 {
    let Some(masks) = RegisterMasks::of(function, methods, fields, superinstructions) else {
        return crate::bytecode::RegisterRoots::ALL_LIVE_AT_ENTRY;
    };
    let entry = entry_live(function, &masks);
    if !RegisterMasks::too_large_to_keep(function) {
        function.register_roots.keep_solved(masks);
    }
    entry
}

/// `function`'s live registers before each instruction, solved at the width `M`, which holds
/// every one of its registers.
pub(super) fn live_masks<M: LiveMask>(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Vec<M> {
    let narrow = |(read, written): (RegisterSet, RegisterSet)| (M::narrow(read), M::narrow(written));
    let transfers: Vec<_> =
        function.instructions().map(|instruction| narrow(transfer(instruction, methods, fields, superinstructions))).collect();
    analyze_transfers(function, &transfers)
}

/// One past the highest register `live` holds before `function`'s first instruction, and at
/// least its parameters' registers, which a tier's entry reads whether or not the body does;
/// or `RegisterRoots::ALL_LIVE_AT_ENTRY` where its masks bound no register there: a body past
/// the set's width, or one past the liveness budget, whose collection scans every register.
fn entry_live(function: &Function, live: &RegisterMasks) -> u16 {
    if function.registers > ROOT_MAP_REGISTERS || live.is_windowed() {
        return crate::bytecode::RegisterRoots::ALL_LIVE_AT_ENTRY;
    }
    let Some(entry) = live.get(0) else { return crate::bytecode::RegisterRoots::ALL_LIVE_AT_ENTRY };
    (0..function.registers).rev().find(|&register| register_in(entry, usize::from(register))).map_or(0, |register| register + 1).max(function.params)
}

/// Whether the `Call` `instruction` at `pc` writes its result to a register (no result flags)
/// that is dead after the call, under `live`: a lane call may let the callee skip building a
/// result nothing reads (`ImmediateLayout::LANE_CALL_RESULT_UNUSED_FLAG`).
pub fn call_result_dead(instruction: WideInstruction, pc: usize, live: &RegisterMasks) -> bool {
    instruction.op().traits().has(crate::bytecode::Traits::PLAIN_CALL)
        && instruction.result_flags() == 0
        && live.get(pc + 1).is_some_and(|after| register_dead(after, usize::from(instruction.a() & crate::bytecode::REGISTER_MASK)))
}

pub(crate) fn analyze(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Option<Vec<RegisterSet>> {
    buffers::analyze_with(function, methods, fields, superinstructions, &mut crate::compile::CompileScratch::default())
}

/// Each instruction's (uses, definitions) register masks, for a function whose registers fit
/// the masks; liveness iterates only the live sets over this fixed table.
pub(super) fn transfers(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Option<Vec<(RegisterSet, RegisterSet)>> {
    let mut table = Vec::new();
    buffers::transfers_into(function, methods, fields, superinstructions, &mut table).then_some(table)
}

/// Every instruction's transfers, at the set's width: a body past it is solved per window.
pub(super) fn all_transfers(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Vec<(RegisterSet, RegisterSet)> {
    function
        .instructions()
        .map(|instruction| transfer(instruction, methods, fields, superinstructions))
        .collect()
}

/// One instruction's (uses, definitions) register masks, the entry `transfers` records for it.
pub fn transfer(
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

/// The registers live after the instruction at `pc`: those live before any of its successors.
pub(super) fn live_out(successors: &crate::bytecode::Successors, live: &[RegisterSet], pc: usize) -> RegisterSet {
    successors.at(pc).fold(RegisterSet::default(), |output, (successor, _)| output | live.get(successor).copied().unwrap_or_default())
}

/// The transfer of an op whose field layout alone decides it: each of A, B and C a register
/// it reads, writes or both, or a field naming no register; no register window or pair,
/// operand, field base, optional register, numeric local target, method site or
/// superinstruction (`uses`, `definitions`). Most instructions are of such ops.
#[derive(Clone, Copy)]
struct FixedTransfer {
    /// The fields, in `InstructionField::ALL` order, whose register the op reads.
    reads: [bool; 3],
    /// The fields, in that order, whose register the op writes.
    writes: [bool; 3],
    reads_result: bool,
    has_result: bool,
}

impl FixedTransfer {
    /// `op`'s fixed transfer, when its layout decides one.
    const fn of(op: Op) -> Option<Self> {
        if matches!(
            op.immediate_layout(),
            ImmediateLayout::CallWindow
                | ImmediateLayout::CallWindowWithEvalFlags
                | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags
                | ImmediateLayout::RegisterPair
        ) || matches!(op.immediate_role(), ImmediateRole::MethodSiteIndex | ImmediateRole::SuperinstructionIndex)
        {
            return None;
        }
        let mut reads = [false; 3];
        let mut writes = [false; 3];
        let mut index = 0;
        while index < InstructionField::ALL.len() {
            let layout = op.field_layout(InstructionField::ALL[index]);
            match layout {
                FieldLayout::RegisterWindowBase
                | FieldLayout::Operand
                | FieldLayout::NumericIndexOperand
                | FieldLayout::FieldBase
                | FieldLayout::OptionalRegister
                | FieldLayout::OptionalReadRegister
                | FieldLayout::ConstructArguments
                | FieldLayout::NumericLocalTarget => return None,
                _ => {}
            }
            reads[index] = layout.reads_register();
            writes[index] = layout.writes_register();
            index += 1;
        }
        Some(Self {
            reads,
            writes,
            reads_result: op.result_layout().reads_result_register(),
            has_result: op.has_result_register(),
        })
    }

    /// The registers of `instruction`'s fields that `flags` marks.
    #[inline(always)]
    fn registers(instruction: WideInstruction, flags: [bool; 3]) -> RegisterSet {
        let mut set = RegisterSet::default();
        let mut index = 0;
        while index < flags.len() {
            if flags[index] {
                set |= register_bit(instruction.field_value(InstructionField::ALL[index]));
            }
            index += 1;
        }
        set
    }

    #[inline(always)]
    fn uses(self, instruction: WideInstruction) -> RegisterSet {
        let result = if self.reads_result { register_bit(instruction.result_register()) } else { RegisterSet::default() };
        Self::registers(instruction, self.reads) | result
    }

    #[inline(always)]
    fn definitions(self, instruction: WideInstruction) -> RegisterSet {
        let result = if self.has_result && !instruction.returns_from_frame() && !instruction.writes_numeric_local() {
            register_bit(instruction.result_register())
        } else {
            RegisterSet::default()
        };
        Self::registers(instruction, self.writes) | result
    }
}

/// `FixedTransfer::of` of every op, by op.
const FIXED_TRANSFERS: [Option<FixedTransfer>; Op::COUNT] = {
    let mut table = [None; Op::COUNT];
    let mut index = 0;
    while index < Op::COUNT {
        if let Some(op) = Op::from_index(index) {
            table[index] = FixedTransfer::of(op);
        }
        index += 1;
    }
    table
};

pub(super) fn uses(
    instruction: WideInstruction,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> RegisterSet {
    if let Some(fixed) = FIXED_TRANSFERS[instruction.op() as usize] {
        return fixed.uses(instruction);
    }
    general_uses(instruction, methods, fields, superinstructions)
}

/// `uses` of an instruction whose op has no `FixedTransfer`.
fn general_uses(
    instruction: WideInstruction,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> RegisterSet {
    let field_reads = InstructionField::ALL
        .iter()
        .copied()
        .fold(RegisterSet::default(), |mask, field| {
            mask | field_uses(instruction, field, fields)
        });
    let result = if instruction.op().result_layout().reads_result_register() {
        register_bit(instruction.result_register())
    } else {
        RegisterSet::default()
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
        _ => RegisterSet::default(),
    };
    let indexed_uses = match instruction.op().immediate_role() {
        ImmediateRole::MethodSiteIndex => method_arguments(instruction, methods),
        ImmediateRole::SuperinstructionIndex => superinstructions
            [instruction.superinstruction_index()]
        .wide
        .iter()
        .fold(RegisterSet::default(), |mask, &nested| mask | uses(nested, methods, fields, superinstructions)),
        _ => RegisterSet::default(),
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
                InstructionField::A => return RegisterSet::default(),
            };
            operand(input_operand.0, fields)
        }
        FieldLayout::FieldBase => field_base(instruction, fields),
        FieldLayout::OptionalReadRegister => instruction.optional_register_b().map_or_else(RegisterSet::default, register_bit),
        FieldLayout::ConstructArguments => match instruction.construct_arguments() {
            crate::bytecode::ConstructArguments::Registers(window) => {
                range(window.base, window.count)
            }
            crate::bytecode::ConstructArguments::Array(register) => register_bit(register),
        },
        _ => RegisterSet::default(),
    }
}

pub fn definitions(
    instruction: WideInstruction,
    superinstructions: &[Superinstruction],
) -> RegisterSet {
    if let Some(fixed) = FIXED_TRANSFERS[instruction.op() as usize] {
        return fixed.definitions(instruction);
    }
    general_definitions(instruction, superinstructions)
}

/// `definitions` of an instruction whose op has no `FixedTransfer`.
fn general_definitions(
    instruction: WideInstruction,
    superinstructions: &[Superinstruction],
) -> RegisterSet {
    let result = if instruction.op().has_result_register()
        && !instruction.returns_from_frame()
        && !instruction.writes_numeric_local()
    {
        register_bit(instruction.result_register())
    } else {
        RegisterSet::default()
    };
    let field_writes = InstructionField::ALL
        .iter()
        .copied()
        .fold(RegisterSet::default(), |mask, field| {
            mask | field_definitions(instruction, field)
        });
    let packed_definitions = match instruction.op().immediate_layout() {
        ImmediateLayout::RegisterPair => {
            let (first, second) = instruction.register_pair();
            register_bit(first) | register_bit(second)
        }
        _ => RegisterSet::default(),
    };
    let indexed_definitions = match instruction.op().immediate_role() {
        ImmediateRole::SuperinstructionIndex => superinstructions
            [instruction.superinstruction_index()]
        .wide
        .iter()
        .fold(RegisterSet::default(), |mask, &nested| mask | definitions(nested, superinstructions)),
        _ => RegisterSet::default(),
    };
    result | field_writes | packed_definitions | indexed_definitions
}

fn field_definitions(instruction: WideInstruction, field: InstructionField) -> RegisterSet {
    let layout = instruction.op().field_layout(field);
    match layout {
        layout if layout.writes_register() => register_bit(field_register(instruction, field)),
        FieldLayout::OptionalRegister => instruction.optional_register_b().map_or_else(RegisterSet::default, register_bit),
        FieldLayout::NumericLocalTarget => instruction
            .numeric_local_store_target()
            .map_or_else(RegisterSet::default, |target| register_bit(target.register)),
        _ => RegisterSet::default(),
    }
}

fn field_register(instruction: WideInstruction, field: InstructionField) -> Register {
    instruction.field_value(field)
}

fn field_base(instruction: WideInstruction, fields: &[FieldSite]) -> RegisterSet {
    match instruction.field_lookup() {
        None => RegisterSet::default(),
        Some(FieldLookup::Site(index)) => fields
            .get(index)
            .and_then(|site| site.base.register_index())
            .map_or_else(RegisterSet::default, register_bit),
        Some(FieldLookup::Atom { base, .. }) => base.register_index().map_or_else(RegisterSet::default, register_bit),
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
            .map_or_else(RegisterSet::default, register_bit);
    }
    RegisterSet::default()
}

fn method_arguments(instruction: WideInstruction, methods: &(impl MethodArguments + ?Sized)) -> RegisterSet {
    methods.arguments(instruction.method_site_index())
}

/// The registers `base..base + count` that the set covers, as `register_bit` of each.
fn range(base: u16, count: u16) -> RegisterSet {
    let start = u32::from(base).min(RegisterSet::BITS);
    let end = (u32::from(base) + u32::from(count)).min(RegisterSet::BITS);
    match end - start {
        0 => RegisterSet::default(),
        span => (RegisterSet::MAX >> (RegisterSet::BITS - span)) << start,
    }
}

/// The set of `register` alone; empty past the set's width.
pub fn register_bit(register: u16) -> RegisterSet {
    RegisterSet::bit(usize::from(register))
}

#[cfg(test)]
mod tests;
