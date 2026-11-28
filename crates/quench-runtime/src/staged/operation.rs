//! The staged operation an instruction denotes: the `Operation` enum, its decode by the catalog
//! decoder for the opcode (`Op::staged`), and its queries over instructions. `staged.rs` re-exports it.

use super::*;
#[cfg(not(stencil_build))]
use super::wasm_jump::{BranchTableWasmJumpExt, WasmJump, WasmJumpWasmJumpExt};
#[cfg(not(stencil_build))]
use crate::bytecode::{Function, ThrownValueSlot};
#[cfg(not(stencil_build))]
use super::{
    superinstructions::ConstArrayPairSuperinstructionsExt, suspend::OperationSuspendExt,
    wasm_access::OperationWasmAccessExt, wasm_state::OperationWasmStateExt,
};
#[cfg(not(stencil_build))]
use super::wasm_float::OperationWasmFloatExt;
#[cfg(not(stencil_build))]
use super::wasm_simd::OperationWasmSimdExt;

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
#[cfg(not(stencil_build))]
mod records;
#[cfg(not(stencil_build))]
pub use records::*;

/// An instruction `step` runs, with the operands a tier decoded.
#[derive(Clone, Copy)]
pub enum Operation<W, P> {
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
    /// `value instanceof constructor` into `destination`, decided by the frame's words of the value
    /// and the constructor (`Frame::instance_of`, the walk `object::instanceof_by_words`); the frame's
    /// default leaves it to the VM's `Binary` callout. Leaves at the miss edge where the walk declines.
    #[allow(dead_code)] // no opcode decodes to it: a stencil row builds it
    InstanceOf { destination: Register, value: Register, constructor: Register },
    /// Continue where register `source` holds a direct array (the array identity test, `Frame::direct_array`);
    /// any other value leaves at the miss edge.
    #[allow(dead_code)] // no opcode decodes to it: a stencil row builds it
    DirectArray { source: Register },
    /// Define `destination` with the `length` of the direct array in register `source` (`Frame::array_length`).
    #[allow(dead_code)] // no opcode decodes to it: a stencil row builds it
    ArrayLength { destination: Register, source: Register },
    /// Define `destination` with the `length` of the typed array in register `source` (`Frame::typed_length`).
    #[allow(dead_code)] // no opcode decodes to it: a stencil row builds it
    TypedLength { destination: Register, source: Register },
    /// Append the value in register `value` to the array in register `array`, where its backing has
    /// room and is writable (`Frame::array_append`); a value the frame does not append leaves at the miss edge.
    #[allow(dead_code)] // no opcode decodes to it: a stencil row builds it
    ArrayAppend { array: Register, value: Register },
    /// The empty object, object or array literal, constant array, regexp literal, closure,
    /// literal template or string concatenation `allocation` makes, defined into `destination`; a due collection leaves
    /// at the frame's miss edge (`Frame::allocate_value`).
    Allocate { destination: Register, allocation: Allocation, proof: P },
    /// A `SuperConstArrayObject2` record's pair (`superinstructions`): a constant array, two `Binary`
    /// operations, then an object pair, each part's row in turn (`superinstructions::step`).
    #[cfg(not(stencil_build))]
    SuperConstArrayObject2(super::superinstructions::ConstArrayPair<P>),
    /// Define element `index` of the literal array in `array` as `value` (`DefineArrayElement`), a
    /// fresh hole of that array; the frame's element definer answers it (`Frame::define_element`).
    DefineElement { array: Register, index: u32, value: Register },
    /// Continue unless an interrupt or a due collection is requested (`Frame::safepoint`).
    Safepoint { proof: P },
    /// Continue where the word `source` holds exactly `expected`, bit for bit (`guard::identity`).
    #[allow(dead_code)] // no opcode decodes to it: a tier row builds it
    GuardIdentity { source: W, expected: u64 },
    /// Continue where the code's invalidation word is zero (`guard::live`, `Frame::invalidation`).
    #[allow(dead_code)] // no opcode decodes to it: a tier row builds it
    GuardLive,
    /// Continue where the active realm's global object word is `expected` (`guard::realm`).
    #[allow(dead_code)] // no opcode decodes to it: a tier row builds it
    GuardRealm { expected: u64 },
    /// Continue where the language bits the tag of `source` decides meet `mask` (`guard::declared`).
    #[allow(dead_code)] // no opcode decodes to it: a tier row builds it
    GuardDeclared { source: W, mask: u64 },
    /// Continue where `source` is a direct object of exact `shape` whose prototype link is `link`, its record found in
    /// the object record table `records` (`guard::prototype`, `entry::shape_link_matches`).
    #[allow(dead_code)] // no opcode decodes to it: a tier row builds it
    GuardPrototype { source: W, records: W, shape: u64, link: u64 },
    /// Continue where `source` is a direct object of exact `shape`, its record found in the object record table
    /// `records`, in the storage `form` the entry expects (`guard::shape`): a region's entry test of a loop-invariant
    /// receiver, whose shape and form its accesses then keep (`base_region::proofs`).
    #[allow(dead_code)] // no opcode decodes to it: a tier row builds it
    GuardShape { source: W, records: W, shape: u64, form: super::object::Form },
    /// Continue where `source` is a direct object whose live class cell's class is `class`, which fixes its exact shape and
    /// prototype link (`guard::class`). The receiver is the cell, read as a flat run (`object::cell_fields`), no record.
    #[allow(dead_code)] // no opcode decodes to it: a tier row builds it
    GuardClass { source: W, class: u32 },
    /// Continue where the run stores specialized, the dispatch word `specialized` (`guard::stores`): a region's entry test
    /// of the stores it admits without a test of their own (`base_region::proofs`).
    #[allow(dead_code)] // no opcode decodes to it: a tier row builds it
    GuardStores { specialized: W },
    /// A taken backward edge: poll under `proof`, then count it (`guard::back_edge`).
    #[allow(dead_code)] // no opcode decodes to it: a tier row builds it
    BackEdge { proof: P },
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
    /// The VM's unary `operator` (a `UnaryOperator`, or an internal unary test) of register `source`, defining
    /// `destination` (`CallOut::Unary`): an operator no staged row decides.
    Unary { destination: Register, source: Register, operator: u32 },
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
    /// Define `destination` with the script-top-level binding the declaration `declaration` names (its
    /// position in the root's global lexicals), through cache `site`; a temporal dead zone is the VM's to raise.
    GlobalLexicalLoad { destination: Register, declaration: u32, site: u16 },
    /// The store of `source` to the script-top-level binding the declaration `declaration` names, through
    /// cache `site`, an initialization where `initialize`; the VM makes it (const and dead zone rules included).
    GlobalLexicalStore { source: Register, declaration: u32, site: u16, initialize: bool },
    /// An iteration or spread step (`Protocol`) over the `operands` registers, defining
    /// `destination` where it has one; the generic protocol runs guest code, a callout.
    Protocol { step: Protocol, destination: Option<Register>, operands: [Option<Register>; 2] },
    /// Define `destination` with the next key of the for-in state in register `state`; continue
    /// at the jump target once none is left.
    ForInNext { destination: Register, state: Register },
    /// `base[key]` in the body of the for-in whose state register is `state`: the element read the VM
    /// makes (the keyed fast arm needs the collection epoch, which no frame offers now).
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
    #[cfg(not(stencil_build))]
    Suspend { kind: super::Suspension, destination: Register, value: Register },
    /// Continue: a record a rewrite emptied (`Op::Nop`), which reads and writes nothing.
    Nop,
    /// The value `base.name` reads through `lookup`'s plan (`property::read_with`); the tier
    /// defines the result register.
    GetField { base: FieldBase, lookup: FieldLookup, destination: Register },
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
    ElementRead { base: Register, key: Operand, destination: Register },
    /// An indexed read whose base is a frame local, not a register (`GetIndex` over a numeric
    /// local operand): the tier's exact path makes it (`element_rows::step` leaves it unresolved).
    ElementReadOperand { destination: Register, base: Operand, key: Operand },
    /// A numeric increment or decrement whose result goes to the frame local `local` instead of a
    /// register (`IncDec` with a numeric local target): the tier's exact path makes it.
    IncDecLocal { local: Register, source: Register, decrement: bool },
    /// `base[key] = value`, replacing a present dense element in place where the array owns its
    /// backing alone and the value needs no barrier log, else the VM's store.
    ElementWrite { value: Register, base: Register, key: Register, strict: bool },
    /// Define `destination` from the capture `slot`, `depth` scopes out, or from the root
    /// environment's projected binding where `root` (`Frame::capture_slot`). The decode never
    /// names a root: only a tier that found the walk end at one does.
    LoadCapture { destination: Register, depth: u16, slot: u16, root: bool },
    /// Store `source` to the capture `slot`, `depth` scopes out (`Frame::store_capture`).
    StoreCapture { source: Register, depth: u16, slot: u16 },
    /// `left` compared with `right` (`Comparison`), jumping to the target when the comparison
    /// holds, or fails where `negate` (`JumpBinaryFalse`); otherwise continue after it.
    CompareJump { comparison: Comparison, left: Operand, right: Operand, negate: bool },
    /// A Wasm float, float conversion or select (`wasm_float`); the frame's Wasm words decide it.
    #[cfg(not(stencil_build))]
    WasmFloat(wasm_float::WasmFloatOperation),
    /// A Wasm i32 or i64 register or immediate operator (`wasm_int`), which the row answers from its words.
    #[cfg(not(stencil_build))]
    WasmInt(wasm_int::WasmIntOperation),
    /// A Wasm i32 conditional jump or fused add-and-jump over int32 operands (`wasm_jump`).
    #[cfg(not(stencil_build))]
    WasmJump(WasmJump),
    /// A Wasm branch table: control goes to the entry its index picks (`Edge::Entry`, `wasm_jump`).
    #[cfg(not(stencil_build))]
    BranchTable(BranchTable),
    /// A Wasm global's load or store, or a register window's fill (`wasm_state`).
    #[cfg(not(stencil_build))]
    WasmState(wasm_state::WasmStateOperation),
    /// A Wasm direct load or store of the function's memory (`wasm_access`).
    #[cfg(not(stencil_build))]
    WasmAccess(wasm_access::WasmAccessOperation),
    /// A Wasm SIMD operator or byte shuffle on the lane's vector registers (`wasm_simd`).
    #[cfg(not(stencil_build))]
    WasmVector(wasm_simd::VectorOperation),
    /// The tier prologue of a frameless activation of `function` with `parameters` parameters
    /// (`TierPrologue`, a lane's synthetic record): the JavaScript tier enters it, and the activation
    /// continues where that completes it (`prologue::PrologueFrame::tier_prologue`).
    Prologue { function: u32, parameters: u16 },
    /// The prologue of a specialized body of `function` (`BodyEntry`, a lane's synthetic record): the body
    /// runs on the activation's registers, binding `this` where `binds_this` (`prologue::PrologueFrame::body_entry`).
    BodyEntry { function: u32, binds_this: bool },
}

/// Each `Operation` variant as a number, for the dispatchers' compile-time choice of arm (`step_of` and the other
/// `*_of` steps): a caller that knows its variant passes it, and every other arm is dropped before code generation
/// (an `if const` test); `ANY` runs every arm, as the interpreter does.
pub mod kind {
    pub const ANY: u16 = 0;
    pub const MOVE: u16 = 1;
    pub const LOAD_CONST: u16 = 2;
    pub const LOAD_LOCAL_PLAIN: u16 = 3;
    pub const STORE_LOCAL_PLAIN: u16 = 4;
    pub const LOAD_LOCAL_CHECKED: u16 = 5;
    pub const STORE_LOCAL_CHECKED: u16 = 6;
    pub const INITIALIZE_LOCAL: u16 = 7;
    pub const JUMP: u16 = 8;
    pub const JUMP_IF: u16 = 9;
    pub const LOAD_THIS: u16 = 10;
    pub const LOAD_CALLEE: u16 = 11;
    pub const NUMBER: u16 = 12;
    pub const REQUIRE_OBJECT_COERCIBLE: u16 = 13;
    pub const REQUIRE_ITERATOR_RESULT: u16 = 14;
    pub const LOGICAL_NOT: u16 = 15;
    pub const TYPEOF_INDEX: u16 = 16;
    pub const TYPEOF_TEST: u16 = 17;
    pub const TYPEOF_STRING: u16 = 18;
    pub const INDEX_WRITE: u16 = 19;
    /// Retired with the object-record add site (S7l): no operation takes it. The number stays, so the shard table keeps its positions.
    pub const ADD_SITE: u16 = 20;
    /// Retired with `Operation::NamingSite` (S7j4a): no operation takes it. The number stays, so the shard table keeps its positions.
    pub const NAMING_SITE: u16 = 21;
    pub const INSTANCE_OF: u16 = 22;
    pub const DIRECT_ARRAY: u16 = 23;
    pub const ARRAY_LENGTH: u16 = 24;
    pub const TYPED_LENGTH: u16 = 25;
    pub const ARRAY_APPEND: u16 = 26;
    pub const ALLOCATE: u16 = 27;
    pub const SUPER_CONST_ARRAY_OBJECT2: u16 = 28;
    pub const DEFINE_ELEMENT: u16 = 29;
    pub const SAFEPOINT: u16 = 30;
    pub const GUARD_IDENTITY: u16 = 31;
    pub const GUARD_LIVE: u16 = 32;
    pub const GUARD_REALM: u16 = 33;
    pub const GUARD_DECLARED: u16 = 34;
    pub const GUARD_PROTOTYPE: u16 = 35;
    pub const GUARD_SHAPE: u16 = 36;
    pub const GUARD_STORES: u16 = 37;
    pub const BACK_EDGE: u16 = 38;
    pub const INDEX_SET_HELPER: u16 = 39;
    pub const LOAD_NAME: u16 = 40;
    pub const VOID: u16 = 41;
    pub const UNARY: u16 = 42;
    pub const PROPERTY_KEY: u16 = 43;
    pub const EQUALITY: u16 = 44;
    pub const BINARY: u16 = 45;
    pub const LOAD_ENV_LOCAL: u16 = 46;
    pub const STORE_ENV_LOCAL: u16 = 47;
    pub const ARGUMENTS_LENGTH: u16 = 48;
    pub const ARGUMENT_GET: u16 = 49;
    pub const CLONE_ENV: u16 = 50;
    pub const GLOBAL_TYPEOF: u16 = 51;
    pub const SET_FUNCTION_NAME: u16 = 52;
    pub const GLOBAL_WRITE: u16 = 53;
    pub const GLOBAL_LEXICAL_LOAD: u16 = 54;
    pub const GLOBAL_LEXICAL_STORE: u16 = 55;
    pub const PROTOCOL: u16 = 56;
    pub const FOR_IN_NEXT: u16 = 57;
    pub const ENUMERATED_GET: u16 = 58;
    pub const FIELD_STORE_MISS: u16 = 59;
    pub const ITERATOR_CLEANUP_PUSH: u16 = 60;
    pub const ITERATOR_CLEANUP_POP: u16 = 61;
    pub const STRING_CASE_INDEX: u16 = 62;
    pub const SUSPEND: u16 = 63;
    pub const NOP: u16 = 64;
    pub const GET_FIELD: u16 = 65;
    pub const SET_FIELD: u16 = 66;
    pub const STORE_FIELD: u16 = 67;
    pub const FIELD_GET_HOST: u16 = 68;
    pub const FIELD_ADD: u16 = 69;
    pub const DELETE: u16 = 70;
    pub const INDEX_GET_HOST: u16 = 71;
    pub const CALL: u16 = 72;
    pub const SPREAD_CALL: u16 = 73;
    pub const SUPER_CALL: u16 = 74;
    pub const RETURN: u16 = 75;
    pub const THROW: u16 = 76;
    pub const ELEMENT_READ: u16 = 77;
    pub const ELEMENT_READ_OPERAND: u16 = 78;
    pub const INC_DEC_LOCAL: u16 = 79;
    pub const ELEMENT_WRITE: u16 = 80;
    pub const LOAD_CAPTURE: u16 = 81;
    pub const STORE_CAPTURE: u16 = 82;
    pub const COMPARE_JUMP: u16 = 83;
    pub const WASM_FLOAT: u16 = 84;
    pub const WASM_INT: u16 = 85;
    pub const WASM_JUMP: u16 = 86;
    pub const BRANCH_TABLE: u16 = 87;
    pub const WASM_STATE: u16 = 88;
    pub const WASM_ACCESS: u16 = 89;
    pub const WASM_VECTOR: u16 = 90;
    pub const PROLOGUE: u16 = 91;
    pub const BODY_ENTRY: u16 = 92;
    pub const GUARD_CLASS: u16 = 93;

    /// Whether a dispatcher instantiated for `kind` runs an arm of `variants`.
    pub const fn admits(kind: u16, variants: &[u16]) -> bool {
        if kind == ANY {
            return true;
        }
        let mut at = 0;
        while at < variants.len() {
            if variants[at] == kind {
                return true;
            }
            at += 1;
        }
        false
    }

    /// Whether a dispatcher instantiated for `kind` runs a catch-all arm after arms for `variants`.
    pub const fn outside(kind: u16, variants: &[u16]) -> bool {
        kind == ANY || !admits(kind, variants)
    }
}


impl<W, P> Operation<W, P> {
    /// The operation as a root read where it is a `LoadCapture` (`root`), else unchanged: a row whose
    /// record a walk settled on a root binding decodes its read so (`FLAG_ROOT`).
    pub fn rooted(self) -> Self {
        match self {
            Self::LoadCapture { destination, depth, slot, .. } => Self::LoadCapture { destination, depth, slot, root: true },
            other => other,
        }
    }

    /// Where a `Throw` at `pc` of `function` goes when a handler of the function takes it natively: a catch that binds nothing
    /// jumps to the handler's block, one that binds a register of the `registers`-register activation moves the thrown value
    /// first. `None` for any other operation, and for a throw the interpreter's unwinding settles (a generator, an async
    /// function, a dynamic scope, a `with` scope to unwind, a handler that does not open after the throw, a local binding).
    #[cfg(not(stencil_build))]
    pub fn handler_edge(&self, function: &Function, pc: u32, registers: Register) -> Option<HandlerEdge> {
        let Self::Throw { source } = self else { return None };
        if *source >= registers || !unwinds_to_handlers(function) {
            return None;
        }
        let handler = function.handler_at(pc).filter(|handler| handler.with_depth == 0 && handler.target > pc)?;
        match handler.slot {
            None => Some(HandlerEdge::Jump { target: handler.target }),
            Some(ThrownValueSlot::Register(slot)) if slot < registers => Some(HandlerEdge::Bind { slot, target: handler.target }),
            Some(ThrownValueSlot::Register(_) | ThrownValueSlot::Local(_)) => None,
        }
    }
}

/// The native edge of a covered `throw` (`Operation::handler_edge`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandlerEdge {
    /// A catch that binds nothing: control jumps to the handler's block.
    Jump { target: u32 },
    /// A catch that binds the thrown value to the register `slot`, then jumps to the handler's block.
    Bind { slot: Register, target: u32 },
}

/// Whether the exceptions of `function` reach its handlers by the interpreter's unwinding rule alone: a generator, an async
/// function and a dynamic scope have state only its own unwinding settles.
#[cfg(not(stencil_build))]
fn unwinds_to_handlers(function: &Function) -> bool {
    !(function.is_generator || function.is_async || function.has_dynamic_scope_access())
}


