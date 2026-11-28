//! Runtime glue of `operation`: the decode of an operation from an instruction (`Op::staged` names
//! the decoder), the instruction queries the tiers ask, and the row constants the decode names.
//! These were inherent methods of `Operation` in the moved file; each is a trait method now (rule
//! 4.3), so a caller imports the trait that owns it.
#![allow(unused_imports)]
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::allocation::AllocationAllocationExt;
use super::*;
use crate::staged::{InvocationFormsExt, OperationElementRowsExt, OperationGenericExt, OperationOperationRecordsExt};

/// The compare-and-jump row: a jump on the failing comparison of two operands, which `decode_generic`
/// reads as `CompareJump` (`decode_compare`). A tier names the row by this constant.
pub const COMPARE_JUMP_ROW: u16 = Op::JumpBinaryFalse as u16;

/// The method-call row: a call whose callee is a property of its receiver, through a method site, which
/// `decode_call` reads as a call site (`Invocation::of`, its `method` form). A tier names the row by this constant.
pub const METHOD_CALL_ROW: u16 = Op::CallMethod as u16;

/// The decode of an `Operation` from an instruction, by the decoder the catalog names for its opcode
/// (`Op::staged`), and the operations of each decoder.
pub trait OperationOperationExt<W, P>: Sized {
    fn decode(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self>;

    fn direct_helper(&self) -> bool;

    fn decode_wasm_jump(instruction: WideInstruction) -> Option<Self>;

    fn decode_wasm_int(instruction: WideInstruction) -> Option<Self>;

    fn decode_completed(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<(Self, crate::bytecode::Register)>;

    fn decode_literal_completed_with(instruction: WideInstruction, parts: Option<[WideInstruction; 4]>, proof: P)
    -> Option<(Self, crate::bytecode::Register)>;

    fn completion_split(instruction: WideInstruction) -> Option<(WideInstruction, crate::bytecode::Register)>;

    fn decode_constant_load(instruction: WideInstruction) -> Option<(Register, usize)>;

    fn decode_names(instruction: WideInstruction) -> Option<Self>;

    const DEFINE_ELEMENT_OP: Op;

    fn decode_literal(instruction: WideInstruction, proof: P) -> Option<Self>;

    fn decode_literal_with(instruction: WideInstruction, parts: Option<[WideInstruction; 4]>, proof: P) -> Option<Self>;

    fn decode_flow<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self>;

    fn encode_move(a: Register, b: Register) -> WideInstruction;

    fn encode_jump(target: u32) -> WideInstruction;

    fn encode_nop() -> WideInstruction;

    fn decode_call(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self>;

    fn encode_call(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction;

    fn encode_construct(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction;

    fn decode_property(instruction: WideInstruction) -> Option<Self>;

    fn decode_get_field(instruction: WideInstruction) -> Option<Self>;

    fn encode_get_field(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction;

    fn encode_set_field(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction;

    fn encode_set_this_field(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction;

    fn stores_field(record: WideInstruction, value: Register) -> bool;

    fn decode_value(instruction: WideInstruction) -> Option<Self>;

    fn encode_load_capture(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction;

    fn decode_unary(instruction: WideInstruction) -> Self;

    fn number_unary(destination: Register, source: Register, operation: crate::vm::operations::NumberUnary) -> Self;

    fn decode_local(instruction: WideInstruction) -> Option<Self>;
}

impl<W, P> OperationOperationExt<W, P> for Operation<W, P> {
    /// The staged operation `instruction` denotes, with the operands the catalog fixes and the
    /// tables of `context` its operands index, by the decoder the catalog names for its opcode
    /// (`Op::staged`). `None` where the opcode has no staged operation, or where its constant or
    /// method site is not available to this tier: the caller then takes its own exact path. The
    /// heap operations are the frame's (`Frame::field_read`); a tier decodes those itself.
    #[inline(always)]
    fn decode(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
        match instruction.op().staged()? {
            Decoder::Flow => Self::decode_flow(instruction, context),
            Decoder::Local => Self::decode_local(instruction),
            Decoder::Generic => Self::decode_generic(instruction),
            Decoder::Value => Self::decode_value(instruction),
            Decoder::Property => Self::decode_property(instruction),
            Decoder::Element => Self::decode_element(instruction),
            Decoder::Call => Self::decode_call(instruction, context),
            Decoder::Names => names::decode(instruction),
            Decoder::WasmFloat => Self::decode_wasm_float(instruction),
            // Under the tier's proof of the allocation (`decode_literal`), so none here.
            Decoder::Proved | Decoder::Literal => None,
            Decoder::WasmInt => Self::decode_wasm_int(instruction),
            Decoder::WasmJump => Self::decode_wasm_jump(instruction),
            Decoder::WasmState => Self::decode_wasm_state(instruction, context),
            Decoder::WasmMemory => Self::decode_wasm_access(instruction, context),
            Decoder::WasmVector => Self::decode_wasm_vector(instruction, context),
            // A lane row is the lane's own (`decode_lane`): the plan keeps its generic or exit route for it.
            Decoder::Lane => None,
        }
    }

    /// Whether the packed step helper runs this operation directly (`dispatch_fast::family_helpers`): its
    /// direct arm reads the operands from the running frame and declines, before any effect, where the
    /// general step decides. Only the operations whose arms are shared with `Vm::step` qualify
    /// (`write_binary_result`; an element arm declines on any non-dense, non-typed element; field sites
    /// have no direct arm yet).
    fn direct_helper(&self) -> bool {
        matches!(self, Self::Binary { .. } | Self::ElementRead { .. } | Self::ElementWrite { .. }
            | Self::GetField { .. } | Self::SetField { .. } | Self::LogicalNot { .. } | Self::TypeofIndex { .. })
    }

    /// The Wasm i32 jumps of `decode` (`Decoder::WasmJump`), from any record's wide fields
    /// (`Fields::wide`).
    #[inline(always)]
    fn decode_wasm_jump(instruction: WideInstruction) -> Option<Self> {
        WasmJump::decode(instruction).map(Self::WasmJump).or_else(|| BranchTable::decode(instruction).map(Self::BranchTable))
    }

    /// The Wasm integer operations of `decode` (`Decoder::WasmInt`), from a record's wide fields.
    #[inline(always)]
    fn decode_wasm_int(instruction: WideInstruction) -> Option<Self> {
        wasm_int::decode(instruction).map(Self::WasmInt)
    }

    /// The staged operation `instruction` makes apart from how its result completes the frame
    /// (its result flags: a return, a `this` write), and those flags. A tier that completes a
    /// record's result itself asks this instead of naming the opcode; `decode` alone stays the
    /// operation with no completion.
    #[inline(always)]
    fn decode_completed(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<(Self, crate::bytecode::Register)> {
        let (plain, flags) = Self::completion_split(instruction)?;
        Some((Self::decode(plain, context)?, flags))
    }

    /// `decode_completed` of the literal operations (`decode_literal_with`) under the tier's `proof`, with the
    /// superinstruction parts `parts` of `decode_literal_with`: the operation and the flags its record completes
    /// the frame by.
    fn decode_literal_completed_with(instruction: WideInstruction, parts: Option<[WideInstruction; 4]>, proof: P)
        -> Option<(Self, crate::bytecode::Register)> {
        let (plain, flags) = Self::completion_split(instruction)?;
        Some((Self::decode_literal_with(plain, parts, proof)?, flags))
    }

    /// The record `instruction` is without its completion flags, and those flags: a return, or a `this` write.
    #[inline(always)]
    fn completion_split(instruction: WideInstruction) -> Option<(WideInstruction, crate::bytecode::Register)> {
        use crate::bytecode::{RETURN_REGISTER, SET_THIS_REGISTER};
        // The returned flags are the completions only: a return, or a `this` write. A numeric local target
        // is where the result goes, not how it completes, so it is left to the record (`writes_numeric_local`);
        // it shares its bit with the `this` write, and only the layout tells them apart.
        let layout = instruction.op().result_layout();
        let completing = (layout.allowed_flags() & RETURN_REGISTER)
            | if matches!(layout, crate::bytecode::ResultLayout::ReturnableAndThis) { SET_THIS_REGISTER } else { 0 };
        let flags = instruction.result_flags() & completing;
        if instruction.result_flags() & !layout.allowed_flags() != 0 { return None; }
        // Every allowed flag is a completion or the numeric local target, which the row's caller writes
        // (`writes_numeric_local`), so the plain record carries none of them.
        let plain = if instruction.result_flags() != 0 { instruction.without_result_flags() } else { instruction };
        Some((plain, flags))
    }

    /// The destination and constant index of an instruction `decode_flow` reads as `LoadConst`,
    /// for a tier that holds its constants in a table of its own (the b-jit's Numbers): the
    /// same decode, with the constant left for the tier to look up.
    #[inline(always)]
    fn decode_constant_load(instruction: WideInstruction) -> Option<(Register, usize)> {
        (instruction.op() == Op::LoadConst).then(|| (instruction.result_register(), instruction.constant_index()))
    }

    /// The environment, global and iteration operations of `decode` (`Decoder::Names`), from a
    /// record's wide fields (`Fields::wide`).
    #[inline(always)]
    fn decode_names(instruction: WideInstruction) -> Option<Self> {
        names::decode(instruction)
    }

    /// The opcode of an element definition, which `decode_literal_with` reads as `DefineElement`
    /// (its `DefineArrayElement` arm): the row a tier's literal step matches on.
    const DEFINE_ELEMENT_OP: Op = Op::DefineArrayElement;

    /// The literal operations of `decode` (`Decoder::Literal`) under the tier's `proof` of the
    /// allocation: the allocation a literal, closure or constant array record makes
    /// (`Allocation::decode`, which an array literal's element definitions do not change: the
    /// array is a hole array and each definition is `DefineElement`), and an element definition.
    #[inline(always)]
    fn decode_literal(instruction: WideInstruction, proof: P) -> Option<Self> {
        Self::decode_literal_with(instruction, None, proof)
    }

    /// `decode_literal`, where `parts` are the four records of the superinstruction a
    /// `SuperConstArrayObject2` record names (the program's table entry); `None` for any other record,
    /// and for a pair whose `parts` are none.
    #[inline]
    fn decode_literal_with(instruction: WideInstruction, parts: Option<[WideInstruction; 4]>, proof: P) -> Option<Self> {
        match instruction.op() {
            Op::DefineArrayElement => Some(Self::DefineElement {
                array: instruction.register_b(), index: instruction.imm(), value: instruction.register_a() }),
            Op::SuperConstArrayObject2 => {
                let pair = crate::staged::superinstructions::ConstArrayPair::decode(instruction, parts?, proof)?;
                Some(Self::SuperConstArrayObject2(pair))
            }
            _ => {
                let (destination, allocation) = Allocation::decode(instruction, &[])?;
                Some(Self::Allocate { destination, allocation, proof })
            }
        }
    }

    /// The register moves and control transfers of `decode`, from any record a tier holds
    /// (`Fields`): the one flow decoder, the bytecode's and the lane's alike.
    #[inline(always)]
    fn decode_flow<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        Some(match instruction.op() {
            Op::Await | Op::Yield | Op::YieldStar => return Self::decode_suspend(instruction.wide()),
            Op::Move => Self::Move {
                destination: instruction.result_register(), source: instruction.register_b() },
            Op::LoadConst => Self::LoadConst {
                destination: instruction.result_register(),
                constant: instruction.constant(context)? },
            Op::Jump => Self::Jump,
            Op::JumpFalse => Self::JumpIf {
                register: instruction.register_a(), taken: false },
            Op::JumpTrue => Self::JumpIf {
                register: instruction.register_a(), taken: true },
            Op::Throw => Self::Throw { source: instruction.register_a() },
            Op::Nop => Self::Nop,
            _ => Self::Return { source: instruction.register_a() },
        })
    }

    /// The record the `Op::Move` arm of `decode_flow` reads as `Move { destination: a, source: b }`:
    /// that arm's inverse, which a tier's rewrite writes a copy with.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn encode_move(a: Register, b: Register) -> WideInstruction {
        let record = WideInstruction::new(Op::Move, a, b, 0, 0);
        debug_assert!(matches!(Operation::<u64, ()>::of(record),
            Some(Operation::Move { destination, source }) if destination == record.result_register() && source == b));
        record
    }

    /// The record the `Op::Jump` arm of `decode_flow` reads as `Jump`, to the PC `target` (the field
    /// the decode leaves to the dispatcher).
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn encode_jump(target: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::Jump, 0, 0, 0, target);
        debug_assert!(matches!(Operation::<u64, ()>::of(record), Some(Operation::Jump)) && record.imm() == target);
        record
    }

    /// The record the `Op::Nop` arm of `decode_flow` reads as `Nop`: a marker a rewrite leaves or a
    /// slot a residual clears.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn encode_nop() -> WideInstruction {
        let record = WideInstruction::new(Op::Nop, 0, 0, 0, 0);
        debug_assert!(matches!(Operation::<u64, ()>::of(record), Some(Operation::Nop)));
        record
    }

    /// The call operations of `decode`: a call or construction through its forms (`Invocation`),
    /// except `super(...)`, whose arguments the VM gathers (`spread`). Inlined: a call row decodes
    /// its record on every run, which its constant opcode folds only once the decode is in place.
    #[inline(always)]
    fn decode_call(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
        if instruction.op() == Op::Construct && instruction.is_super_construct() {
            return spread::decode(instruction);
        }
        Some(Self::Call(CallOperation::Site(Invocation::of(instruction, context)?)))
    }

    /// The plain call record `decode_call` reads as a call site: callee in `b`, result in `a`,
    /// receiver and arguments from `c` as `imm` lays them out.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn encode_call(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::Call, a, b, c, imm);
        debug_assert!(matches!(Operation::<u64, ()>::of(record), Some(Operation::Call(_))));
        record
    }

    /// The construction record `decode_call` reads, over the operands of an original construction.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn encode_construct(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::Construct, a, b, c, imm);
        debug_assert!(matches!(Operation::<u64, ()>::of(record),
            Some(Operation::Call(_) | Operation::SuperCall { .. })));
        record
    }

    /// The property operations of `decode`: a plain named read, a named store and `delete`.
    #[inline(always)]
    fn decode_property(instruction: WideInstruction) -> Option<Self> {
        match instruction.op() {
            Op::GetField => Self::decode_get_field(instruction),
            Op::Delete => Some(Self::Delete { destination: instruction.result_register(), base: instruction.register_b(),
                key: instruction.register_c(), strict: instruction.boolean_flag()? }),
            op if op.is_field_store() => Some(Self::SetField { value: instruction.register_a(),
                base: instruction.field_store_register(), atom: instruction.atom_index(),
                site: instruction.cache_site_index(), strict: op.is_strict_field_store() }),
            _ => None,
        }
    }

    /// A named read that completes no frame and writes no `this`; a nested lookup's base is the
    /// tier's own (`FieldBase::NESTED`).
    #[inline(always)]
    fn decode_get_field(instruction: WideInstruction) -> Option<Self> {
        if instruction.result_flags() != 0 { return None; }
        let lookup = instruction.field_lookup()?;
        let base = match lookup {
            FieldLookup::Atom { base, .. } => base,
            FieldLookup::Site(_) => FieldBase(FieldBase::NESTED),
        };
        Some(Self::GetField { base, lookup, destination: instruction.result_register() })
    }

    /// The record `decode_get_field` reads as `GetField`: a named read into `a` through the base in
    /// `b`, the lookup in `c` and the atom in `imm`. A record whose `a` carries result flags is one
    /// the decode leaves to the frame, as it leaves any such read.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn encode_get_field(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::GetField, a, b, c, imm);
        debug_assert!(record.result_flags() != 0 || record.field_lookup().is_none() || matches!(Operation::<u64, ()>::of(record),
            Some(Operation::GetField { destination, .. }) if destination == record.result_register()));
        record
    }

    /// The record the field-store arm of `decode_property` reads as `SetField` of `a` into the
    /// object in `b`, through the lookup in `c` and atom `imm`.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn encode_set_field(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::SetField, a, b, c, imm);
        debug_assert!(Self::stores_field(record, a));
        record
    }

    /// The record the field-store arm of `decode_property` reads as the `SetField` of `a` into the
    /// `this` field named by the site in `c` and atom `imm`: the sink of a fused read.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn encode_set_this_field(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::SetThisField, a, b, c, imm);
        debug_assert!(Self::stores_field(record, a));
        record
    }

    /// Whether `record` decodes as the field store of `value` with the atom and site it holds.
    #[cfg(not(stencil_build))]
    fn stores_field(record: WideInstruction, value: Register) -> bool {
        matches!(Operation::<u64, ()>::of(record), Some(Operation::SetField { value: stored, atom, site, .. })
            if stored == value && atom == record.atom_index() && site == record.cache_site_index())
    }

    /// The value-level operations of `decode`.
    #[inline(always)]
    fn decode_value(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::LoadThis => Self::LoadThis {
                destination: instruction.result_register() },
            Op::LoadCallee => Self::LoadCallee {
                destination: instruction.result_register() },
            Op::RequireObjectCoercible => Self::RequireObjectCoercible {
                register: instruction.register_b() },
            Op::RequireIteratorResult => Self::RequireIteratorResult {
                register: instruction.register_b() },
            Op::ToNumeric => Self::Number(NumberOperation::ToNumeric {
                destination: instruction.result_register(), source: instruction.register_b() }),
            Op::IncDec => match instruction.numeric_local_target() {
                None => Self::Number(NumberOperation::IncDec {
                    destination: instruction.result_register(), source: instruction.register_b(),
                    decrement: instruction.boolean_flag()?, overflow: Overflow::Exit }),
                Some(local) => Self::IncDecLocal { local, source: instruction.register_b(),
                    decrement: instruction.boolean_flag()? },
            },
            Op::LoadCapture if instruction.result_flags() == 0 => Self::LoadCapture {
                destination: instruction.result_register(), depth: instruction.capture_depth(), slot: instruction.capture_slot(),
                root: false },
            Op::StoreCapture => Self::StoreCapture {
                source: instruction.register_a(), depth: instruction.capture_depth(), slot: instruction.capture_slot() },
            Op::Unary => Self::decode_unary(instruction),
            _ => return None,
        })
    }

    /// The record the `Op::LoadCapture` arm of `decode_value` reads as `LoadCapture` into `a` at the
    /// depth and slot `imm` names; a record whose `a` carries result flags the decode leaves to the frame.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn encode_load_capture(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::LoadCapture, a, b, c, imm);
        debug_assert!(record.result_flags() != 0 || matches!(Operation::<u64, ()>::of(record),
            Some(Operation::LoadCapture { destination, depth, slot, .. })
                if destination == record.result_register() && depth == record.capture_depth() && slot == record.capture_slot()));
        record
    }

    /// The unary operators of `decode_value`.
    #[inline(always)]
    fn decode_unary(instruction: WideInstruction) -> Self {
        let (destination, source) = (instruction.result_register(), instruction.register_b());
        let operator = instruction.unary_operator();
        if let Some((index, negated)) = crate::bytecode::typeof_test(operator) {
            return Self::TypeofTest { destination, source, index: index as u64, negated };
        }
        match operator {
            #[cfg(not(stencil_build))]
            _ if operator == crate::vm::operations::unary_operator::LogicalNot => Self::LogicalNot { destination, source },
            #[cfg(not(stencil_build))]
            _ if operator == crate::vm::operations::unary_operator::Typeof => Self::TypeofString { destination, source },
            _ if operator == crate::bytecode::UNARY_TYPEOF_INDEX => Self::TypeofIndex { destination, source },
            #[cfg(not(stencil_build))]
            _ if operator == crate::vm::operations::unary_operator::BitwiseNot =>
                Self::Number(NumberOperation::BitwiseNot { destination, source: Operand::register(source) }),
            #[cfg(not(stencil_build))]
            _ if operator == crate::vm::operations::unary_operator::UnaryNegation =>
                Self::number_unary(destination, source, crate::vm::operations::NumberUnary::Negate),
            #[cfg(not(stencil_build))]
            _ if operator == crate::vm::operations::unary_operator::UnaryPlus =>
                Self::number_unary(destination, source, crate::vm::operations::NumberUnary::Plus),
            #[cfg(not(stencil_build))]
            _ if operator == crate::vm::operations::unary_operator::Void => Self::Void { destination },
            _ => Self::Unary { destination, source, operator },
        }
    }

    /// `operation` of a Number `source` in the int32 domain: an int32 operand decides it, a double
    /// or a value no Number leaves to the frame's miss edge (`Unary`'s arm).
    #[inline(always)]
    fn number_unary(destination: Register, source: Register, operation: crate::vm::operations::NumberUnary) -> Self {
        Self::Number(NumberOperation::Unary { destination, source: Operand::register(source), operation, domain: Domain::Int32 })
    }

    /// The local-slot operations of `decode`.
    #[inline(always)]
    fn decode_local(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::LoadLocalPlain => Self::LoadLocalPlain {
                destination: instruction.result_register(),
                slot: window_slot(instruction.imm()) },
            Op::StoreLocalPlain => Self::StoreLocalPlain {
                source: instruction.register_a(),
                slot: window_slot(instruction.imm()),
                result: instruction.optional_register_b() },
            Op::LoadLocal => Self::LoadLocalChecked {
                destination: instruction.result_register(),
                slot: window_slot(instruction.imm()),
                numeric: instruction.result_flags() & crate::bytecode::NUMERIC_LOCAL_TARGET != 0 },
            Op::StoreLocal => Self::StoreLocalChecked {
                source: instruction.register_a(),
                slot: window_slot(instruction.imm()),
                result: instruction.optional_register_b(),
                initializing: instruction.c() != 0 },
            Op::InitializeTdz => Self::InitializeLocal {
                slot: window_slot(instruction.imm()) },
            _ => return None,
        })
    }
}
