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
    /// Continue at the jump target.
    Jump,
    /// Continue at the jump target when register `register` is truthy, or when it is falsy
    /// where `taken` is false; otherwise continue after this instruction.
    JumpIf { register: Register, taken: bool },
    /// Define `destination` with the activation's `this`; where the activation does not
    /// hold it, leave at the frame's miss edge.
    LoadThis { destination: Register },
    /// Continue: a record a rewrite emptied (`Op::Nop`), which reads and writes nothing.
    Nop,
    /// Raise the value of register `source` as the frame's completion (`Frame::raise`).
    Throw { source: Register },
    /// Define `destination` from local slot `slot` with its TDZ check; `numeric` marks a numeric local target.
    LoadLocalChecked { destination: Register, slot: u16, numeric: bool },
    /// Write `source` to local slot `slot` with its TDZ check, and to `result` where the instruction has one;
    /// `initializing` ends the slot's TDZ.
    StoreLocalChecked { source: Register, slot: u16, result: Option<Register>, initializing: bool },
    /// Reset local slot `slot` to its temporal dead zone (`InitializeTdz`).
    InitializeLocal { slot: u16 },
    /// Push the iterator in register `iterator` with its `done` register on the cleanup stack.
    IteratorCleanupPush { iterator: Register, done: Register },
    /// Pop the running activation's top cleanup entry.
    IteratorCleanupPop,
    /// The case of `source` among the `count` string constants from `start`, defined into `destination`.
    StringCaseIndex { destination: Register, source: Register, start: u32, count: u16 },
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
    /// Store `value` at dense array element `index`, under `proof`.
    IndexWrite { index: W, value: W, proof: P },
    /// The empty object, object or array literal, constant array, regexp literal, closure,
    /// literal template or string concatenation `allocation` makes, defined into `destination`; a due collection leaves
    /// at the frame's miss edge (`Frame::allocate_value`).
    Allocate { destination: Register, allocation: Allocation, proof: P },
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
    /// ToPropertyKey of a string `source`, which is its own key; any other value leaves at the
    /// miss edge, where the VM converts it (`Vm::to_property_key`: a Number allocates, an object
    /// runs guest code).
    PropertyKey { destination: Register, source: Register },
    /// `left == right` (or `===`; negated for `!=`, `!==`) where the two words decide it: the
    /// same word, two int32s, or `null` and `undefined` under `==`. Any other pair leaves at the
    /// miss edge, where the VM compares (strings, BigInts, conversions that run guest code).
    Equality { destination: Register, left: Register, right: Register, strict: bool, negated: bool },
    /// Any other `Binary` of two registers (a string or BigInt relation, an arithmetic of other
    /// than Numbers, `instanceof`, `in`): the VM's own operation, a callout.
    Binary { destination: Register, left: Register, right: Register, operator: u32 },
    /// Define `destination` from environment slot `slot` (`environment::environment_slot`); a
    /// sparse storage or an internal marker leaves at the frame's miss edge.
    LoadEnvLocal { destination: Register, slot: u16 },
    /// Write `source` to environment slot `slot`, and to `result` where the instruction has one;
    /// `initialize` where the store is the binding's declaration.
    StoreEnvLocal { source: Register, slot: u16, result: Option<Register>, initialize: bool },
    /// Define `destination` with the length of the arguments binding in local `slot`.
    ArgumentsLength { destination: Register, slot: u16 },
    /// Define `destination` with the `typeof` name of the global `atom` through cache `site`
    /// (the lane's own read); an unresolvable name is `undefined`, not an error.
    GlobalTypeof { destination: Register, atom: u32, site: u16 },
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
    /// `left` compared with `right` (`Comparison`), jumping to the target when the comparison
    /// holds, or fails where `negate` (`JumpBinaryFalse`); otherwise continue after it.
    CompareJump { comparison: Comparison, left: Operand, right: Operand, negate: bool },
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
