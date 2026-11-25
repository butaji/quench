//! The staged operation an instruction denotes: the `Operation` enum, its decode by the catalog
//! decoder for the opcode (`Op::staged`), and its queries over instructions. `staged.rs` re-exports it.

use super::*;
#[cfg(not(stencil_build))]
use super::wasm_jump::WasmJump;

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
    /// A property add of `value` to the object at the record table `records`, through a site's
    /// transition `edge` (and the prototype `link` a chained site must hold), storing it in `form`
    /// (`add::site_add`). The tier names the object word and the site's words: a register's or the
    /// activation's receiver, so one variant serves the register and `this` add rows. No opcode
    /// decodes to it; a stencil row builds it. Leaves at the miss edge where the add declines.
    #[allow(dead_code)] // no opcode decodes to it: a stencil row builds it
    AddSite { records: W, object: W, edge: add::Edge<W>, link: Option<W>, form: Form, value: W },
    /// A function's inferred name `name` written to the function word `function`'s empty name slot
    /// through a site's naming edge (`naming::site_name`, the `(edge, empty)` pair). No opcode decodes
    /// to it; a stencil row builds it. Leaves at the miss edge where the naming declines.
    #[allow(dead_code)] // no opcode decodes to it: a stencil row builds it
    NamingSite { records: W, function: W, naming: (add::Edge<W>, W), form: Form, name: W },
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
    pub const ADD_SITE: u16 = 20;
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

    /// Whether a dispatcher instantiated for `kind` runs an arm of `variants`.
    pub(crate) const fn admits(kind: u16, variants: &[u16]) -> bool {
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
    pub(crate) const fn outside(kind: u16, variants: &[u16]) -> bool {
        kind == ANY || !admits(kind, variants)
    }
}

/// The compare-and-jump row: a jump on the failing comparison of two operands, which `decode_generic`
/// reads as `CompareJump` (`decode_compare`). A tier names the row by this constant.
#[cfg(not(stencil_build))]
pub const COMPARE_JUMP_ROW: u16 = Op::JumpBinaryFalse as u16;

/// The method-call row: a call whose callee is a property of its receiver, through a method site, which
/// `decode_call` reads as a call site (`Invocation::of`, its `method` form). A tier names the row by this constant.
#[cfg(not(stencil_build))]
pub const METHOD_CALL_ROW: u16 = Op::CallMethod as u16;

/// Decoding reads instructions and the opcode catalog, which the stencil build (`quench-bjit`) does
/// not have: a stencil row builds its operation from its holes instead.
#[cfg(not(stencil_build))]
impl<W, P> Operation<W, P> {
    /// The staged operation `instruction` denotes, with the operands the catalog fixes and the
    /// tables of `context` its operands index, by the decoder the catalog names for its opcode
    /// (`Op::staged`). `None` where the opcode has no staged operation, or where its constant or
    /// method site is not available to this tier: the caller then takes its own exact path. The
    /// heap operations are the frame's (`Frame::field_read`); a tier decodes those itself.
    #[inline(always)]
    pub fn decode(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
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
    /// general step decides. Only the operations whose arms are shared with `Vm::step` and whose miss the
    /// site's feedback can gate qualify (`write_binary_result`; element sites record `OperandKinds::DENSE`
    /// and `TYPED`; field sites have no direct arm yet).
    pub fn direct_helper(&self) -> bool {
        matches!(self, Self::Binary { .. } | Self::ElementRead { .. } | Self::ElementWrite { .. }
            | Self::GetField { .. } | Self::SetField { .. } | Self::LogicalNot { .. } | Self::TypeofIndex { .. })
    }

    /// The Wasm i32 jumps of `decode` (`Decoder::WasmJump`), from any record's wide fields
    /// (`Fields::wide`).
    #[inline(always)]
    pub fn decode_wasm_jump(instruction: WideInstruction) -> Option<Self> {
        WasmJump::decode(instruction).map(Self::WasmJump).or_else(|| BranchTable::decode(instruction).map(Self::BranchTable))
    }

    /// The Wasm integer operations of `decode` (`Decoder::WasmInt`), from a record's wide fields.
    #[inline(always)]
    pub fn decode_wasm_int(instruction: WideInstruction) -> Option<Self> {
        wasm_int::decode(instruction).map(Self::WasmInt)
    }

    /// The staged operation `instruction` makes apart from how its result completes the frame
    /// (its result flags: a return, a `this` write), and those flags. A tier that completes a
    /// record's result itself asks this instead of naming the opcode; `decode` alone stays the
    /// operation with no completion.
    #[inline(always)]
    pub fn decode_completed(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<(Self, crate::bytecode::Register)> {
        let (plain, flags) = Self::completion_split(instruction)?;
        Some((Self::decode(plain, context)?, flags))
    }

    /// `decode_completed` of the literal operations (`decode_literal_with`) under the tier's `proof`, with the
    /// superinstruction parts `parts` of `decode_literal_with`: the operation and the flags its record completes
    /// the frame by.
    pub fn decode_literal_completed_with(instruction: WideInstruction, parts: Option<[WideInstruction; 4]>, proof: P)
        -> Option<(Self, crate::bytecode::Register)> {
        let (plain, flags) = Self::completion_split(instruction)?;
        Some((Self::decode_literal_with(plain, parts, proof)?, flags))
    }

    /// The record `instruction` is without its completion flags, and those flags: a return, or a `this` write.
    #[inline(always)]
    pub(crate) fn completion_split(instruction: WideInstruction) -> Option<(WideInstruction, crate::bytecode::Register)> {
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
    pub fn decode_constant_load(instruction: WideInstruction) -> Option<(Register, usize)> {
        (instruction.op() == Op::LoadConst).then(|| (instruction.result_register(), instruction.constant_index()))
    }

    /// The environment, global and iteration operations of `decode` (`Decoder::Names`), from a
    /// record's wide fields (`Fields::wide`).
    #[inline(always)]
    pub fn decode_names(instruction: WideInstruction) -> Option<Self> {
        names::decode(instruction)
    }

    /// The opcode of an element definition, which `decode_literal_with` reads as `DefineElement`
    /// (its `DefineArrayElement` arm): the row a tier's literal step matches on.
    pub const DEFINE_ELEMENT_OP: Op = Op::DefineArrayElement;

    /// The literal operations of `decode` (`Decoder::Literal`) under the tier's `proof` of the
    /// allocation: the allocation a literal, closure or constant array record makes
    /// (`Allocation::decode`, which an array literal's element definitions do not change: the
    /// array is a hole array and each definition is `DefineElement`), and an element definition.
    #[inline(always)]
    pub fn decode_literal(instruction: WideInstruction, proof: P) -> Option<Self> {
        Self::decode_literal_with(instruction, None, proof)
    }

    /// `decode_literal`, where `parts` are the four records of the superinstruction a
    /// `SuperConstArrayObject2` record names (the program's table entry); `None` for any other record,
    /// and for a pair whose `parts` are none.
    #[inline]
    pub fn decode_literal_with(instruction: WideInstruction, parts: Option<[WideInstruction; 4]>, proof: P) -> Option<Self> {
        match instruction.op() {
            Op::DefineArrayElement => Some(Self::DefineElement {
                array: instruction.register_b(), index: instruction.imm(), value: instruction.register_a() }),
            Op::SuperConstArrayObject2 => {
                let pair = super::superinstructions::ConstArrayPair::decode(instruction, parts?, proof)?;
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
    pub fn decode_flow<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
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
    pub fn encode_move(a: Register, b: Register) -> WideInstruction {
        let record = WideInstruction::new(Op::Move, a, b, 0, 0);
        debug_assert!(matches!(Operation::<u64, ()>::of(record),
            Some(Operation::Move { destination, source }) if destination == record.result_register() && source == b));
        record
    }

    /// The record the `Op::Jump` arm of `decode_flow` reads as `Jump`, to the PC `target` (the field
    /// the decode leaves to the dispatcher).
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub fn encode_jump(target: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::Jump, 0, 0, 0, target);
        debug_assert!(matches!(Operation::<u64, ()>::of(record), Some(Operation::Jump)) && record.imm() == target);
        record
    }

    /// The record the `Op::Nop` arm of `decode_flow` reads as `Nop`: a marker a rewrite leaves or a
    /// slot a residual clears.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub fn encode_nop() -> WideInstruction {
        let record = WideInstruction::new(Op::Nop, 0, 0, 0, 0);
        debug_assert!(matches!(Operation::<u64, ()>::of(record), Some(Operation::Nop)));
        record
    }

    /// The call operations of `decode`: a call or construction through its forms (`Invocation`),
    /// except `super(...)`, whose arguments the VM gathers (`spread`). Inlined: a call row decodes
    /// its record on every run, which its constant opcode folds only once the decode is in place.
    #[inline(always)]
    pub fn decode_call(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
        if instruction.op() == Op::Construct && instruction.is_super_construct() {
            return spread::decode(instruction);
        }
        Some(Self::Call(CallOperation::Site(Invocation::of(instruction, context)?)))
    }

    /// The plain call record `decode_call` reads as a call site: callee in `b`, result in `a`,
    /// receiver and arguments from `c` as `imm` lays them out.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub fn encode_call(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::Call, a, b, c, imm);
        debug_assert!(matches!(Operation::<u64, ()>::of(record), Some(Operation::Call(_))));
        record
    }

    /// The construction record `decode_call` reads, over the operands of an original construction.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub fn encode_construct(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::Construct, a, b, c, imm);
        debug_assert!(matches!(Operation::<u64, ()>::of(record),
            Some(Operation::Call(_) | Operation::SuperCall { .. })));
        record
    }

    /// Whether an opcode index is the plain call `encode_call` writes: the stop the inliner takes
    /// over once the call site is hot.
    #[inline(always)]
    pub const fn is_call_index(opcode: u16) -> bool {
        opcode == Op::Call as u16
    }

    /// The property operations of `decode`: a plain named read, a named store and `delete`.
    #[inline(always)]
    pub fn decode_property(instruction: WideInstruction) -> Option<Self> {
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
    pub fn encode_get_field(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::GetField, a, b, c, imm);
        debug_assert!(record.result_flags() != 0 || record.field_lookup().is_none() || matches!(Operation::<u64, ()>::of(record),
            Some(Operation::GetField { destination, .. }) if destination == record.result_register()));
        record
    }

    /// The record the field-store arm of `decode_property` reads as `SetField` of `a` into the
    /// object in `b`, through the lookup in `c` and atom `imm`.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub fn encode_set_field(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
        let record = WideInstruction::new(Op::SetField, a, b, c, imm);
        debug_assert!(Self::stores_field(record, a));
        record
    }

    /// The record the field-store arm of `decode_property` reads as the `SetField` of `a` into the
    /// `this` field named by the site in `c` and atom `imm`: the sink of a fused read.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub fn encode_set_this_field(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
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
    pub fn decode_value(instruction: WideInstruction) -> Option<Self> {
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
    pub fn encode_load_capture(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
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
    pub fn decode_local(instruction: WideInstruction) -> Option<Self> {
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
    pub fn rooted(self) -> Self {
        match self {
            Self::LoadCapture { destination, depth, slot, .. } => Self::LoadCapture { destination, depth, slot, root: true },
            other => other,
        }
    }
}

// The instruction queries the tiers ask: runtime only, as the stencils decode no instruction.
#[cfg(not(stencil_build))]
impl Operation<u64, ()> {
    /// The operation `instruction` denotes, decoded with no tables (`DecodeContext::EMPTY`): what a
    /// tier asks an instruction's meaning by, where the row's operands need no table.
    pub fn of(instruction: WideInstruction) -> Option<Self> {
        Self::decode(instruction, DecodeContext::EMPTY)
    }

    /// The lane's own rows (`Decoder::Lane`): a string-case index over the constants the record names, and an
    /// `await` the lane's frame suspends.
    #[inline(always)]
    pub fn decode_lane(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::StringCaseIndex => Self::StringCaseIndex { destination: instruction.result_register(),
                source: instruction.register_c(), start: instruction.imm(), count: instruction.element_count() },
            Op::TierPrologue => Self::Prologue { function: instruction.imm(), parameters: instruction.a() },
            Op::BodyEntry => Self::BodyEntry { function: instruction.imm(), binds_this: instruction.b() != 0 },
            _ => return None,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    /// Whether `instruction` is the named store `SetField` itself (not a store to `this`).
    pub(crate) fn is_set_field(instruction: WideInstruction) -> bool {
        instruction.op() == Op::SetField
    }

    /// Whether `instruction` does nothing: a marker that defines and writes nothing.
    pub fn is_nop(instruction: WideInstruction) -> bool {
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
    pub fn is_plain_call(instruction: WideInstruction) -> bool {
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
    pub fn is_throw(instruction: WideInstruction) -> bool {
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
    pub fn is_field_read(instruction: WideInstruction) -> bool {
        instruction.op() == Op::GetField
    }

    /// Whether `instruction` is a named field store (`SetField`, `SetThisField`, or a store of a
    /// field of `this`), whatever its operand.
    pub fn is_field_store(instruction: WideInstruction) -> bool {
        instruction.op().is_field_store()
    }

    /// The field cache sites `instruction` reads (`true`) or stores through (`false`), as the
    /// program's field sites name them: a named read's lookup (its site, or the sites of a
    /// nested lookup), and a field store's cache site.
    pub fn field_cache_sites(instruction: WideInstruction, field_sites: &[crate::bytecode::FieldSite]) -> Vec<(u16, bool)> {
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
    pub fn is_unit_rewritten(instruction: WideInstruction) -> bool {
        instruction.op().is_unit_rewritten()
    }
}

// Queries the runtime's profile and VM ask of the rows.
#[cfg(not(stencil_build))]
impl Operation<u64, ()> {
    /// The row that completes the activation (`Return`). `decode_flow` decodes it as its fallback arm
    /// (`Self::Return`, `invoke::ret`); the profile's tests name a second physical row by it.
    pub const RETURN_ROW: u16 = Op::Return as u16;
}
