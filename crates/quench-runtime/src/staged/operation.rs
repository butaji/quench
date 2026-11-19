//! The staged operation an instruction denotes: the `Operation` enum, its decode by the catalog
//! decoder for the opcode (`Op::staged`), and its queries over instructions. `staged.rs` re-exports it.

use super::*;

/// An instruction `step` runs, with the operands a tier decoded.
#[derive(Clone, Copy)]
pub(crate) enum Operation<W, P> {
    /// Copy register `source` into `destination` with its facts.
    Move { destination: Register, source: Register },
    /// Define `destination` from `constant`.
    LoadConst { destination: Register, constant: Constant },
    /// Define `destination` from local slot `slot`.
    LoadLocalPlain { destination: Register, slot: u16 },
    /// Write `source` to local slot `slot`, and to `result` where the instruction has one.
    StoreLocalPlain { source: Register, slot: u16, result: Option<Register> },
    /// Define `destination` from local slot `slot` where its binding is initialized (LoadLocal): a
    /// binding in its temporal dead zone, or a numeric local target (`numeric`), leaves at the
    /// frame's miss edge. The stencil's LOAD_CHECKED row is this operation with `numeric` false
    /// (`read_initialized_local`); a numeric target is the lane's, not the stencil's.
    LoadLocalChecked { destination: Register, slot: u16, numeric: bool },
    /// Write `source` to local slot `slot` where its binding is initialized, or whatever its state
    /// where `initializing` (a declaration's own store), and to `result` where the instruction has
    /// one (StoreLocal). An assignment to a binding in its temporal dead zone leaves at the miss edge.
    /// The stencil's STORE_CHECKED row is this operation with `initializing` false (`write_initialized_local`);
    /// its LOAD_CHECKED sibling is `LoadLocalChecked`.
    StoreLocalChecked { source: Register, slot: u16, result: Option<Register>, initializing: bool },
    /// Open the temporal dead zone of local slot `slot` by writing the deleted marker (InitializeTdz).
    InitializeLocal { slot: u16 },
    /// Continue at the jump target.
    Jump,
    /// Continue at the jump target when register `register` is truthy, or when it is falsy
    /// where `taken` is false; otherwise continue after this instruction.
    JumpIf { register: Register, taken: bool },
    /// Define `destination` with the activation's `this`; where the activation does not
    /// hold it, leave at the frame's miss edge.
    LoadThis { destination: Register },
    /// Define `destination` with the function the running activation runs (`Frame::callee`); where
    /// the frame does not hold it, leave at the frame's miss edge.
    LoadCallee { destination: Register },
    /// A Number operation (`numeric`).
    Number(NumberOperation),
    /// Continue where register `register` is neither `null` nor `undefined`; those leave at
    /// the frame's miss edge, where the interpreter throws the TypeError
    /// (RequireObjectCoercible).
    RequireObjectCoercible { register: Register },
    /// Continue where register `register` holds an object or array record, which
    /// `RequireIteratorResult` accepts as it is; any other value leaves at the frame's miss edge,
    /// where the VM raises or reads it.
    RequireIteratorResult { register: Register },
    /// Define `destination` with the logical negation of register `source`'s truthiness.
    LogicalNot { destination: Register, source: Register },
    /// Define `destination` with the `typeof` name index (int32) of register `source`'s value
    /// where its own word decides it; a heap value leaves at the frame's miss edge.
    TypeofIndex { destination: Register, source: Register },
    /// Define `destination` with whether `typeof source` is the name at `index` in `TYPEOF_NAMES`,
    /// negated where `negated` (`UNARY_TYPEOF_IS`); a heap value leaves at the frame's miss edge.
    TypeofTest { destination: Register, source: Register, index: u64, negated: bool },
    /// Define `destination` with the `typeof` result string of register `source`'s value (`typeof x`):
    /// the interned string of its name, where the tag decides the name or the frame's kind of a heap
    /// value does (`Frame::type_of`); a value neither decides leaves at the frame's miss edge.
    TypeofString { destination: Register, source: Register },
    /// Store `value` at dense array element `index`, under `proof`.
    IndexWrite { index: W, value: W, proof: P },
    /// The empty object, object or array literal, constant array, regexp literal, closure,
    /// literal template or string concatenation `allocation` makes, defined into `destination`; a due collection leaves
    /// at the frame's miss edge (`Frame::allocate_value`).
    Allocate { destination: Register, allocation: Allocation, proof: P },
    /// Define element `index` of the literal array in `array` as `value` (`DefineArrayElement`), a
    /// fresh hole of that array; the frame's element definer answers it (`Frame::define_element`).
    DefineElement { array: Register, index: u32, value: Register },
    /// Continue unless an interrupt or a due collection is requested (`Frame::safepoint`).
    Safepoint { proof: P },
    /// The element store's host helper for `base[key] = value`, under `proof`; the frame's
    /// miss edge takes a failed store.
    IndexSetHelper { base: W, key: W, value: W, proof: P },
    /// The name read of `atom` through cache `site` into `destination`, and `undefined` into
    /// `receiver` where it is a call's: the frame's facts about the global (`GlobalFrame::global_binding`)
    /// decide it, else the VM's read (`CallOut::LoadName`). A call's receiver is the VM's where
    /// a `with` scope may bind the name, so a read the frame does not resolve leaves.
    LoadName { destination: Register, atom: u32, site: u16, receiver: Option<Register>, proof: P },
    /// Define `destination` as `undefined` (`void`).
    Void { destination: Register },
    /// ToPropertyKey of a string `source`, which is its own key; any other value leaves at the
    /// miss edge, where the VM converts it (`Vm::to_property_key`: a Number allocates, an object
    /// runs guest code).
    PropertyKey { destination: Register, source: Register },
    /// `left == right` (or `===`; negated for `!=`, `!==`) where the two words decide it: the
    /// same word, two int32s, or `null` and `undefined` under `==`. Any other pair leaves at the
    /// miss edge, where the VM compares (strings, BigInts, conversions that run guest code).
    Equality { destination: Register, left: Operand, right: Operand, strict: bool, negated: bool },
    /// Any other `Binary` of two operands, each a register or a program constant (a string or BigInt
    /// relation, an arithmetic of other than Numbers, `instanceof`, `in`): the VM's own operation, a
    /// callout.
    Binary { destination: Register, left: Operand, right: Operand, operator: u32 },
    /// Define `destination` from environment slot `slot` (`environment::environment_slot`); a
    /// sparse storage or an internal marker leaves at the frame's miss edge.
    LoadEnvLocal { destination: Register, slot: u16 },
    /// Write `source` to environment slot `slot`, and to `result` where the instruction has one;
    /// `initialize` where the store is the binding's declaration.
    StoreEnvLocal { source: Register, slot: u16, result: Option<Register>, initialize: bool },
    /// Define `destination` with the length of the arguments binding in local `slot`.
    ArgumentsLength { destination: Register, slot: u16 },
    /// Define `destination` with element `index` of the arguments binding in local `slot`: a keeping
    /// activation's kept value, else the frame's or the VM's read (`CallOut::ArgumentGet`).
    ArgumentGet { destination: Register, index: Register, slot: u16 },
    /// Clone the running activation's environment by environment clone plan `plan`: a frame's own
    /// environment, or an environment window's (`CallOut::CloneEnv`). Defines no register.
    CloneEnv { plan: u32 },
    /// Define `destination` with the `typeof` name of the global `atom` through cache `site`
    /// (the lane's own read); an unresolvable name is `undefined`, not an error.
    GlobalTypeof { destination: Register, atom: u32, site: u16 },
    /// SetFunctionName of the function in register `function` to the program atom `name` through cache
    /// `site`: the frame's own naming (`Frame::set_function_name`); a frame that holds none leaves at the
    /// miss edge, where the interpreter names it. Defines no register.
    SetFunctionName { function: Register, name: u32, site: u16 },
    /// The global-name store of `source` to `atom` through cache `site`, an initialization
    /// where `initialize`; a binding the lane's store does not take leaves at the miss edge.
    GlobalWrite { source: Register, atom: u32, site: u16, initialize: bool },
    /// An iteration or spread step (`Protocol`) over the `operands` registers, defining
    /// `destination` where it has one; the generic protocol runs guest code, a callout.
    Protocol { step: Protocol, destination: Option<Register>, operands: [Option<Register>; 2] },
    /// Define `destination` with the next key of the for-in state in register `state`; continue
    /// at the jump target once none is left.
    ForInNext { destination: Register, state: Register },
    /// `base[key]` in the body of the for-in whose state register is `state`: the key's listed
    /// slot where the state's memo answers (`iteration::for_in_keyed_get`), else the element read.
    EnumeratedGet { destination: Register, state: Register, base: Register, key: Register },
    /// The VM field-store helper for `object.atom = value` through cache `site`.
    #[cfg_attr(not(test), allow(dead_code))] // no tier builds it until the call-out rows land
    FieldStoreMiss { object: W, atom: u32, site: u16, value: W, proof: P },
    /// Push the iterator in register `iterator` with its `done` register on the running cleanup
    /// stack (`Frame::iterator_push`), which closes it where the loop exits abruptly.
    IteratorCleanupPush { iterator: Register, done: Register },
    /// Pop the running cleanup stack's top entry (`Frame::iterator_pop`).
    IteratorCleanupPop,
    /// The index of the value in register `source` among the `count` program constants from `start`
    /// (`Frame::case_index`), defined into `destination`: the switch's string-case selector.
    StringCaseIndex { destination: Register, source: Register, start: u32, count: u16 },
    /// Suspend the activation (`Suspension`: `await`, `yield`, `yield*`) with the value of `value`,
    /// resuming with what it receives next in `destination` (`suspend::suspend`, which asks the
    /// frame: `Frame::suspend_await`, `suspend_yield`, `suspend_yield_star`).
    Suspend { kind: super::Suspension, destination: Register, value: Register },
    /// Continue: a record a rewrite emptied (`Op::Nop`), which reads and writes nothing.
    Nop,
    /// The value `base.name` reads through `lookup`'s plan (`property::read_with`); the tier
    /// defines the result register.
    GetField { base: FieldBase, lookup: FieldLookup },
    /// `base.atom = value` through cache `site`'s store plan (`property_write::set_field`);
    /// `base` is `this` for `None`.
    SetField { value: Register, base: Option<Register>, atom: u32, site: u16, strict: bool },
    /// `base.atom = value` of a word no register names, of type `fact`, through cache site
    /// `site`'s store plan (`property_write::store_word`); `base` is `this` for `None`.
    StoreField { base: Option<Register>, value: W, fact: Type, atom: u32, site: u16 },
    /// The VM's read of `atom` of `object`, a value no register names, through `site`
    /// (`PropertyFrame::field_probe`).
    FieldGetHost { object: W, atom: u32, site: u16 },
    /// The VM's adding store of `value` to `atom` of `object` (`PropertyFrame::field_add`).
    #[cfg_attr(not(test), allow(dead_code))]
    FieldAdd { object: W, atom: u32, site: u16, value: W },
    /// `delete base[key]` into `destination`, throwing on a refusal where `strict`
    /// (`PropertyFrame::delete_property`, else `CallOut::Delete`).
    Delete { destination: Register, base: Register, key: Register, strict: bool },
    /// `base[key]` past the dense and typed element reads (`PropertyFrame::index_get_host`).
    #[cfg_attr(not(test), allow(dead_code))]
    IndexGetHost { base: W, key: W },
    /// A step of the call protocol: admission, `this` binding, the callee's window, completion
    /// (`protocol`).
    Call(CallOperation<W>),
    /// A call whose one argument register holds an iterable to spread or an array to apply
    /// (`CallSpread`, `CallArray`): the VM's call, a callout (`spread`).
    SpreadCall { destination: Register, callee: Register, receiver: Register, arguments: Arguments, passing: Passing },
    /// `super(...)`: the VM's construction with the activation's `new.target`, a callout (`spread`).
    SuperCall { destination: Register, callee: Register, arguments: Arguments, passing: Passing },
    /// Complete the activation with the value of `source` (`invoke::ret`).
    Return { source: Register },
    /// Raise the value of register `source` (Throw). No staged row raises it: `step` leaves it
    /// `Unresolved`, so each tier keeps its own throw path.
    Throw { source: Register },
    /// The value `base[key]` reads, a present dense element where the array and the key's int32
    /// are proved (`element_rows`), else the VM's read; the tier defines the result register.
    ElementRead { base: Register, key: Operand },
    /// An indexed read whose base is a frame local, not a register (`GetIndex` over a numeric
    /// local operand): the tier's exact path makes it (`element_rows::step` leaves it unresolved).
    ElementReadOperand { destination: Register, base: Operand, key: Operand },
    /// A numeric increment or decrement whose result goes to the frame local `local` instead of a
    /// register (`IncDec` with a numeric local target): the tier's exact path makes it.
    IncDecLocal { local: Register, source: Register, decrement: bool },
    /// `base[key] = value`, replacing a present dense element in place where the array owns its
    /// backing alone and the value needs no barrier log, else the VM's store.
    ElementWrite { value: Register, base: Register, key: Register },
    /// Define `destination` from the capture `slot`, `depth` scopes out, or from the root
    /// environment's projected binding where `root` (`Frame::capture_slot`). The decode never
    /// names a root: only a tier that found the walk end at one does.
    LoadCapture { destination: Register, depth: u16, slot: u16, root: bool },
    /// Store `source` to the capture `slot`, `depth` scopes out (`Frame::store_capture`).
    StoreCapture { source: Register, depth: u16, slot: u16 },
    /// A Wasm float, float conversion or select (`wasm_float`); the frame's Wasm words decide it, and
    /// a trap or an operand the frame does not hold leaves at the miss edge.
    WasmFloat(wasm_float::WasmFloatOperation),
    /// `left` compared with `right` (`Comparison`), jumping to the target when the comparison
    /// holds, or fails where `negate` (`JumpBinaryFalse`); otherwise continue after it.
    CompareJump { comparison: Comparison, left: Operand, right: Operand, negate: bool },
    /// A Wasm i32 or i64 register or immediate operator (`wasm_int`), which the row answers from its words.
    WasmInt(wasm_int::WasmIntOperation),
    /// A Wasm i32 conditional jump or fused add-and-jump over int32 operands (`wasm_jump`).
    WasmJump(WasmJump),
    /// A Wasm branch table: control goes to the entry its index picks (`Edge::Entry`, `wasm_jump`).
    BranchTable(BranchTable),
    /// A Wasm global's load or store, or a register window's fill (`wasm_state`).
    WasmState(wasm_state::WasmStateOperation),
    /// A Wasm direct load or store of the function's memory (`wasm_access`).
    WasmAccess(wasm_access::WasmAccessOperation),
    /// A Wasm SIMD operator or byte shuffle on the lane's vector registers (`wasm_simd`).
    WasmVector(wasm_simd::VectorOperation),
    /// The tier prologue of a frameless activation of `function` with `parameters` parameters
    /// (`TierPrologue`, a lane's synthetic record): the JavaScript tier enters it, and the activation
    /// continues where that completes it (`prologue::PrologueFrame::tier_prologue`).
    Prologue { function: u32, parameters: u16 },
    /// The prologue of a specialized body of `function` (`BodyEntry`, a lane's synthetic record): the body
    /// runs on the activation's registers, binding `this` where `binds_this` (`prologue::PrologueFrame::body_entry`).
    BodyEntry { function: u32, binds_this: bool },
}
impl<W, P> Operation<W, P> {
    /// The staged operation `instruction` denotes, with the operands the catalog fixes and the
    /// tables of `context` its operands index, by the decoder the catalog names for its opcode
    /// (`Op::staged`). `None` where the opcode has no staged operation, or where its constant or
    /// method site is not available to this tier: the caller then takes its own exact path. The
    /// heap operations are the frame's (`Frame::field_read`); a tier decodes those itself.
    pub(crate) fn decode(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
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

    /// The Wasm i32 jumps of `decode` (`Decoder::WasmJump`), from any record's wide fields
    /// (`Fields::wide`).
    #[inline(always)]
    pub(crate) fn decode_wasm_jump(instruction: WideInstruction) -> Option<Self> {
        WasmJump::decode(instruction).map(Self::WasmJump).or_else(|| BranchTable::decode(instruction).map(Self::BranchTable))
    }

    /// The staged operation `instruction` makes apart from how its result completes the frame
    /// (its result flags: a return, a `this` write), and those flags. A tier that completes a
    /// record's result itself asks this instead of naming the opcode; `decode` alone stays the
    /// operation with no completion.
    pub(crate) fn decode_completed(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<(Self, crate::bytecode::Register)> {
        use crate::bytecode::{RETURN_REGISTER, SET_THIS_REGISTER};
        // A numeric local target is where the result goes, not how it completes: it stays.
        // (It shares its bit with the `this` write: only `ReturnableAndThis` reads that bit as one.)
        let layout = instruction.op().result_layout();
        let completing = (layout.allowed_flags() & RETURN_REGISTER)
            | if matches!(layout, crate::bytecode::ResultLayout::ReturnableAndThis) { SET_THIS_REGISTER } else { 0 };
        let flags = instruction.result_flags() & completing;
        if instruction.result_flags() & !layout.allowed_flags() != 0 { return None; }
        let plain = if flags != 0 { instruction.without_result_flags() } else { instruction };
        Some((Self::decode(plain, context)?, flags))
    }

    /// The destination and constant index of an instruction `decode_flow` reads as `LoadConst`,
    /// for a tier that holds its constants in a table of its own (the b-jit's Numbers): the
    /// same decode, with the constant left for the tier to look up.
    #[inline(always)]
    pub(crate) fn decode_constant_load(instruction: WideInstruction) -> Option<(Register, usize)> {
        (instruction.op() == Op::LoadConst).then(|| (instruction.result_register(), instruction.constant_index()))
    }

    /// The environment, global and iteration operations of `decode` (`Decoder::Names`), from a
    /// record's wide fields (`Fields::wide`).
    #[inline(always)]
    pub(crate) fn decode_names(instruction: WideInstruction) -> Option<Self> {
        names::decode(instruction)
    }

    /// The literal operations of `decode` (`Decoder::Literal`) under the tier's `proof` of the
    /// allocation: the allocation a literal, closure or constant array record makes
    /// (`Allocation::decode`, which an array literal's element definitions do not change: the
    /// array is a hole array and each definition is `DefineElement`), and an element definition.
    #[inline(always)]
    pub(crate) fn decode_literal(instruction: WideInstruction, proof: P) -> Option<Self> {
        match instruction.op() {
            Op::DefineArrayElement => Some(Self::DefineElement {
                array: instruction.register_b(), index: instruction.imm(), value: instruction.register_a() }),
            _ => {
                let (destination, allocation) = Allocation::decode(instruction, &[])?;
                Some(Self::Allocate { destination, allocation, proof })
            }
        }
    }

    /// The register moves and control transfers of `decode`, from any record a tier holds
    /// (`Fields`): the one flow decoder, the bytecode's and the lane's alike.
    #[inline(always)]
    pub(crate) fn decode_flow<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
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

    /// The call operations of `decode`: a call or construction through its forms (`Invocation`),
    /// except `super(...)`, whose arguments the VM gathers (`spread`).
    fn decode_call(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
        if instruction.op() == Op::Construct && instruction.is_super_construct() {
            return spread::decode(instruction);
        }
        Some(Self::Call(CallOperation::Site(Invocation::of(instruction, context)?)))
    }

    /// The property operations of `decode`: a plain named read, a named store and `delete`.
    pub(crate) fn decode_property(instruction: WideInstruction) -> Option<Self> {
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
    fn decode_get_field(instruction: WideInstruction) -> Option<Self> {
        if instruction.result_flags() != 0 { return None; }
        let lookup = instruction.field_lookup()?;
        let base = match lookup {
            FieldLookup::Atom { base, .. } => base,
            FieldLookup::Site(_) => FieldBase(FieldBase::NESTED),
        };
        Some(Self::GetField { base, lookup })
    }

    /// The value-level operations of `decode`.
    #[inline(always)]
    pub(crate) fn decode_value(instruction: WideInstruction) -> Option<Self> {
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
            Op::Unary => return Self::decode_unary(instruction),
            _ => return None,
        })
    }

    /// The unary operators of `decode_value`.
    #[inline(always)]
    fn decode_unary(instruction: WideInstruction) -> Option<Self> {
        let (destination, source) = (instruction.result_register(), instruction.register_b());
        let operator = instruction.unary_operator();
        if let Some((index, negated)) = crate::bytecode::typeof_test(operator) {
            return Some(Self::TypeofTest { destination, source, index: index as u64, negated });
        }
        Some(match operator {
            #[cfg(not(stencil_build))]
            _ if operator == oxc_ast::ast::UnaryOperator::LogicalNot as u32 => Self::LogicalNot { destination, source },
            // A `typeof` that returns from the frame is the general step's (the record returns the value).
            #[cfg(not(stencil_build))]
            _ if operator == oxc_ast::ast::UnaryOperator::Typeof as u32 && !instruction.returns_from_frame() =>
                Self::TypeofString { destination, source },
            _ if operator == crate::bytecode::UNARY_TYPEOF_INDEX => Self::TypeofIndex { destination, source },
            #[cfg(not(stencil_build))]
            _ if operator == oxc_ast::ast::UnaryOperator::BitwiseNot as u32 =>
                Self::Number(NumberOperation::BitwiseNot { destination, source: Operand::register(source) }),
            #[cfg(not(stencil_build))]
            _ if operator == oxc_ast::ast::UnaryOperator::UnaryNegation as u32 =>
                Self::number_unary(destination, source, crate::vm::operations::NumberUnary::Negate),
            #[cfg(not(stencil_build))]
            _ if operator == oxc_ast::ast::UnaryOperator::UnaryPlus as u32 =>
                Self::number_unary(destination, source, crate::vm::operations::NumberUnary::Plus),
            #[cfg(not(stencil_build))]
            _ if operator == oxc_ast::ast::UnaryOperator::Void as u32 && !instruction.returns_from_frame() =>
                Self::Void { destination },
            _ => return None,
        })
    }

    /// `operation` of a Number `source` in the int32 domain: an int32 operand decides it, a double
    /// or a value no Number leaves to the frame's miss edge (`Unary`'s arm).
    #[inline(always)]
    fn number_unary(destination: Register, source: Register, operation: crate::vm::operations::NumberUnary) -> Self {
        Self::Number(NumberOperation::Unary { destination, source: Operand::register(source), operation, domain: Domain::Int32 })
    }

    /// The Wasm integer operations of `decode` (`Decoder::WasmInt`), from a record's wide fields.
    #[inline(always)]
    pub(crate) fn decode_wasm_int(instruction: WideInstruction) -> Option<Self> {
        wasm_int::decode(instruction).map(Self::WasmInt)
    }

    /// The local-slot operations of `decode`.
    #[inline(always)]
    pub(crate) fn decode_local(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::LoadLocalPlain => Self::LoadLocalPlain {
                destination: instruction.result_register(),
                slot: u16::try_from(instruction.local_slot()).ok()? },
            Op::StoreLocalPlain => Self::StoreLocalPlain {
                source: instruction.register_a(),
                slot: u16::try_from(instruction.local_slot()).ok()?,
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

impl<W, P> Operation<W, P> {
    /// The operation as a root read where it is a `LoadCapture` (`root`), else unchanged: a row whose
    /// record a walk settled on a root binding decodes its read so (`FLAG_ROOT`).
    pub(crate) fn rooted(self) -> Self {
        match self {
            Self::LoadCapture { destination, depth, slot, .. } => Self::LoadCapture { destination, depth, slot, root: true },
            other => other,
        }
    }
}

impl Operation<u64, ()> {
    /// The operation `instruction` denotes, decoded with no tables (`DecodeContext::EMPTY`): what a
    /// tier asks an instruction's meaning by, where the row's operands need no table.
    pub(crate) fn of(instruction: WideInstruction) -> Option<Self> {
        Self::decode(instruction, DecodeContext::EMPTY)
    }

    /// The lane's own rows (`Decoder::Lane`): a string-case index over the constants the record names, and an
    /// `await` the lane's frame suspends.
    pub(crate) fn decode_lane(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::StringCaseIndex => Self::StringCaseIndex { destination: instruction.result_register(),
                source: instruction.register_c(), start: instruction.imm(), count: instruction.element_count() },
            Op::TierPrologue => Self::Prologue { function: instruction.imm(), parameters: instruction.register_a() },
            Op::BodyEntry => Self::BodyEntry { function: instruction.imm(), binds_this: instruction.register_b() != 0 },
            _ => return None,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    /// Whether `instruction` is the named store `SetField` itself (not a store to `this`).
    pub(crate) fn is_set_field(instruction: WideInstruction) -> bool {
        instruction.op() == Op::SetField
    }

    /// Whether `instruction` does nothing: a marker that defines and writes nothing.
    pub(crate) fn is_nop(instruction: WideInstruction) -> bool {
        instruction.op() == Op::Nop
    }

    /// Whether `instruction` is a Number binary operation (`NumericBinary`): its operator is the
    /// Number arithmetic, relation or bitwise operation the tier reads from its operands.
    pub(crate) fn is_number_binary(instruction: WideInstruction) -> bool {
        instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
    }

    /// Whether `instruction` is a Number unary operation (`NumericUnary`), whose operator the tier
    /// reads from the instruction's unary operator field.
    pub(crate) fn is_number_unary(instruction: WideInstruction) -> bool {
        instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericUnary
    }

    /// Whether `instruction` is a call through its callee register (`Call`, including the eval
    /// forms that decode to no operation), which an inlined unit plans and admits as a call.
    pub(crate) fn is_plain_call(instruction: WideInstruction) -> bool {
        instruction.op() == Op::Call
    }

    /// Whether `instruction` is an ordinary call through its callee register, with a window or a
    /// spread array of arguments (`Call`, `CallArray`).
    pub(crate) fn is_ordinary_call(instruction: WideInstruction) -> bool {
        instruction.op().is_ordinary_call()
    }

    /// Whether `instruction` constructs (`Construct`), with its callee and arguments in registers.
    pub(crate) fn is_construction(instruction: WideInstruction) -> bool {
        instruction.op() == Op::Construct
    }

    /// Whether `instruction` throws: its decoded operation is `Throw`, which leaves at the miss edge.
    pub(crate) fn is_throw(instruction: WideInstruction) -> bool {
        matches!(Self::of(instruction), Some(Self::Throw { .. }))
    }

    /// Whether `instruction` yields a value from a generator (`Yield`, a lane row). The interpreter
    /// frame answers no suspension (`Frame::suspend_yield`), so its yield is the general loop's step.
    pub(crate) fn is_yield(instruction: WideInstruction) -> bool {
        matches!(Self::decode_lane(instruction), Some(Self::Suspend { kind: super::Suspension::Yield { .. }, .. }))
    }

    /// Whether `instruction` awaits a value (`Await`, a lane row). The interpreter frame answers no
    /// suspension (`Frame::suspend_await`), so an async frame's await is the general loop's step.
    pub(crate) fn is_await(instruction: WideInstruction) -> bool {
        matches!(Self::decode_lane(instruction), Some(Self::Suspend { kind: super::Suspension::Await, .. }))
    }

    /// Whether `instruction` is a named field read (`GetField`), whatever its result flags.
    pub(crate) fn is_field_read(instruction: WideInstruction) -> bool {
        instruction.op() == Op::GetField
    }

    /// Whether `instruction` is a named field store (`SetField`, `SetThisField`, or a store of a
    /// field of `this`), whatever its operand.
    pub(crate) fn is_field_store(instruction: WideInstruction) -> bool {
        instruction.op().is_field_store()
    }

    /// The field cache sites `instruction` reads (`true`) or stores through (`false`), as the
    /// program's field sites name them: a named read's lookup (its site, or the sites of a
    /// nested lookup), and a field store's cache site.
    #[cfg(not(stencil_build))]
    pub(crate) fn field_cache_sites(instruction: WideInstruction, field_sites: &[crate::bytecode::FieldSite]) -> Vec<(u16, bool)> {
        if Self::is_field_read(instruction) {
            return match instruction.field_lookup() {
                Some(FieldLookup::Atom { cache_site, .. }) => vec![(cache_site, true)],
                Some(FieldLookup::Site(index)) => field_sites.get(index).map_or_else(Vec::new, |site| {
                    let reads = [Some(site.first), site.second].into_iter().flatten().map(|(_, cache)| (cache, true));
                    reads.chain(site.sink.map(|(_, cache)| (cache, false))).collect()
                }),
                None => Vec::new(),
            };
        }
        if Self::is_field_store(instruction) { vec![(instruction.cache_site_index(), false)] } else { Vec::new() }
    }

    /// Whether a unit holds `instruction` by its own rewrite or an exit rather than by renaming
    /// (the catalog's `UNIT_REWRITTEN` trait).
    pub(crate) fn is_unit_rewritten(instruction: WideInstruction) -> bool {
        instruction.op().is_unit_rewritten()
    }
}
