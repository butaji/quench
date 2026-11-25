//! The row catalog's macros, and the list of every Number row kind and heap row the b-jit library names,
//! once, in the order their codes are numbered. `catalog!` hands the list to a consumer macro. `row_catalog!`
//! emits either the heap rows' codes (`names`, `row_names.rs`) or the kinds' codes and `HEAP_TABLES`
//! (`tables`, `row_tables.rs`); the runtime's `heap_code_match!` (`base_rows.rs`) reads each heap row's
//! catalog line. Each crate mounts this file once at its root (`#[macro_use]`) before its modules.
//!
//! A heap row is one line: `NAME: uses(dst, a, b, entry),` where `dst`, `a`, `b` are the `Slot`s its
//! operands range over and `entry` is its `Entry` mode, and `x COUNT` makes a family of `COUNT` codes
//! that share the line (a typed kind family). `reserve N` skips N codes no row holds. Rows are listed in
//! chunks, each a `[END TABLE: ...]`, because the positional values chain through the chunks and a
//! single macro expansion over every row would nest deeper than the default recursion limit.
//! Adding a heap row: one line here naming its variant (`= Heap::X`). `Heap::code` in `runtime base_rows.rs` expands
//! these lines (`heap_code_of!`), and the test there lists every variant.

/// Numbers the listed codes consecutively from 0, in the listed order: each `NAME` becomes
/// `pub const NAME: u8` holding the next value, its doc comments attached, and `reserve N` skips
/// `N` values that no code holds. `NAME_COUNT:` before the list also names the count after the last value
/// (one past the highest code). A new code is a new line, never a typed number. `@plain` emits no count.
macro_rules! numbered_codes {
    (@emit $count:ident, $base:expr;) => {
        pub(crate) const $count: u8 = $base;
    };
    (@emit $count:ident, $base:expr; reserve $n:expr $(, $($rest:tt)*)?) => {
        numbered_codes!(@emit $count, $base + $n; $($($rest)*)?);
    };
    (@emit $count:ident, $base:expr; $(#[$m:meta])* $name:ident $(, $($rest:tt)*)?) => {
        $(#[$m])* pub const $name: u8 = $base;
        numbered_codes!(@emit $count, $base + 1; $($($rest)*)?);
    };
    (@plain $base:expr;) => {};
    (@plain $base:expr; reserve $n:expr $(, $($rest:tt)*)?) => {
        numbered_codes!(@plain $base + $n; $($($rest)*)?);
    };
    (@plain $base:expr; $(#[$m:meta])* $name:ident $(, $($rest:tt)*)?) => {
        $(#[$m])* pub const $name: u8 = $base;
        numbered_codes!(@plain $base + 1; $($($rest)*)?);
    };
    ($count:ident: $($list:tt)*) => {
        numbered_codes!(@emit $count, 0; $($list)*);
    };
}

/// One chunk of heap rows: `$start` is the code the chunk's first row takes, `$end` the first code after
/// the chunk, `$table` the chunk's `(first code, count, uses)` entries. `$mode` is `names` (each row's `pub const`)
/// or `tables` (the chunk's entries in `$table`).
macro_rules! heap_chunk {
    ($mode:ident, $end:ident, $table:ident, $start:expr; $($body:tt)*) => {
        heap_chunk!(@go $mode, $end, $table, $start, []; $($body)*);
    };
    (@go $mode:ident, $end:ident, $table:ident, $base:expr, [$($acc:tt)*];) => {
        const $end: u8 = $base;
        heap_chunk!(@table $mode, $table, [$($acc)*]);
    };
    (@table names, $table:ident, [$($acc:tt)*]) => {};
    (@table tables, $table:ident, [$($acc:tt)*]) => {
        pub const $table: &[(u8, u8, Uses)] = &[$($acc)*];
    };
    (@go $mode:ident, $end:ident, $table:ident, $base:expr, [$($acc:tt)*]; reserve $n:expr $(, $($rest:tt)*)?) => {
        heap_chunk!(@go $mode, $end, $table, $base + $n, [$($acc)*]; $($($rest)*)?);
    };
    (@go $mode:ident, $end:ident, $table:ident, $base:expr, [$($acc:tt)*];
        $(#[$m:meta])* $name:ident : unlisted $(, $($rest:tt)*)?) => {
        heap_chunk!(@name $mode, $base, $(#[$m])* $name);
        heap_chunk!(@go $mode, $end, $table, $base + 1, [$($acc)*]; $($($rest)*)?);
    };
    (@go $mode:ident, $end:ident, $table:ident, $base:expr, [$($acc:tt)*];
        $(#[$m:meta])* $name:ident : uses($dst:ident, $a:ident, $b:ident, $entry:ident) $(x $n:ident)? $(= $pat:pat $(=> $off:expr)?)? $(, $($rest:tt)*)?) => {
        heap_chunk!(@name $mode, $base, $(#[$m])* $name);
        heap_chunk!(@go $mode, $end, $table, $base + (1 $(+ ($n) - 1)?),
            [$($acc)* ($base, 1 $(+ ($n) - 1)?, Uses { entry: Entry::$entry, dst: Slot::$dst, a: Slot::$a, b: Slot::$b }),];
            $($($rest)*)?);
    };
    (@name names, $base:expr, $(#[$m:meta])* $name:ident) => {
        $(#[$m])* pub const $name: u8 = $base;
    };
    (@name tables, $base:expr, $(#[$m:meta])* $name:ident) => {};
}

/// The whole catalog: the heap chunks in order (each chained from the one before), then the Number kinds.
/// `names` emits the heap rows' codes; `tables` emits the kinds' codes and `HEAP_TABLES`.
macro_rules! row_catalog {
    (names heap { $($chunk:tt)* } number { $($kinds:tt)* }) => {
        row_catalog!(@heap names, 0, []; $($chunk)*);
    };
    (tables heap { $($chunk:tt)* } number { $($kinds:tt)* }) => {
        row_catalog!(@heap tables, 0, []; $($chunk)*);
        numbered_codes! { @plain 0; $($kinds)* }
    };
    (@heap $mode:ident, $prev:expr, [$($tables:ident,)*];) => {
        row_catalog!(@tail $mode, $prev, [$($tables,)*]);
    };
    (@heap $mode:ident, $prev:expr, [$($tables:ident,)*]; [$end:ident $table:ident: $($body:tt)*] $($rest:tt)*) => {
        heap_chunk!($mode, $end, $table, $prev; $($body)*);
        row_catalog!(@heap $mode, $end, [$($tables,)* $table,]; $($rest)*);
    };
    (@tail names, $prev:expr, [$($tables:ident,)*]) => {
        // The last chunk's end is the heap code count: it fits the `u8` codes.
        const _: () = assert!($prev as usize <= u8::MAX as usize);
    };
    (@tail tables, $prev:expr, [$($tables:ident,)*]) => {
        const _: () = assert!($prev as usize <= u8::MAX as usize);
        pub const HEAP_TABLES: &[&[(u8, u8, Uses)]] = &[$($tables),*];
    };
}

/// The whole catalog, handed to `$consumer` with `$args` ahead of its `heap { .. } number { .. }` body.
#[macro_export]
macro_rules! catalog {
    ($consumer:ident ($($args:tt)*)) => {
        $consumer! { $($args)*
    heap {
    [HEAP_END_1 HEAP_TABLE_1:
    CAPTURE_ROW: uses(Dst, Unread, Unread, Never) = Heap::Capture(_),
    INDEX_ROW: uses(Dst, Unread, Src, Kept) = Heap::Index,
    STORE_ROW: uses(Unread, Src, Src, Kept) = Heap::Store,
    FIELD_ROW: uses(Dst, Unread, Unread, Never) = Heap::Field,
    ARRAY_ROW: uses(Unread, Unread, Unread, Never),
    SITE_ROW: uses(Dst, Unread, Unread, Kept) = Heap::Site | Heap::SiteChain,
    /// The running activation's `this`, a local slot read, a local slot written.
    THIS_ROW: uses(Dst, Unread, Unread, Never) = Heap::This,
    LOAD_LOCAL_ROW: uses(Dst, Unread, Unread, Never) = Heap::LoadLocal(_),
    STORE_LOCAL_ROW: uses(Unread, Src, Unread, Never) = Heap::StoreLocal(_),
    /// The frame slot at hole 1 holds exactly the word at hole 2: the proof a call's callee is the one
    /// a site was stitched for.
    IDENTITY_ROW: uses(Unread, Unread, Unread, Never) = Heap::Identity,
    LENGTH_ROW: uses(Dst, Unread, Unread, Never) = Heap::Length,
    CAPTURE_SET_ROW: uses(Unread, Src, Unread, Never) = Heap::CaptureSet(_),
    SITE_STORE_ROW: uses(Unread, Src, Unread, Kept) = Heap::SiteStore,
    /// The site rows of the activation's receiver (`this`), which the payload holds.
    THIS_SITE_ROW: uses(Dst, Unread, Unread, Kept) = Heap::ThisSite | Heap::ThisChain,
    THIS_STORE_ROW: uses(Unread, Src, Unread, Kept) = Heap::ThisStore,
    /// A prototype-held field folded to the value its holder held when the code was compiled, over the
    /// receiver (shape and prototype link) it stands on; the receiver's variant (`entry`) takes it from the payload.
    HELD_ROW: uses(Dst, Unread, Unread, Kept) = Heap::Held | Heap::ThisHeld,
    /// Leaves unless the invalidation word at the address in hole 1 is zero.
    LIVE_ROW: uses(Unread, Unread, Unread, Never) = Heap::Live,
    /// A fast arm of an instruction whose full operation a helper step makes (`staged::iteration`): the
    /// frame slot at hole 1 holds a direct object or array record (`RequireIteratorResult`), or a direct
    /// string, which is copied to the slot at hole 0 (`ToPropertyKey`). A hit continues at TAKEN, past the
    /// helper step laid out after the row; a miss leaves through EXIT0 into it.
    OBJECT_ROW: uses(Unread, Unread, Unread, Never) = Heap::Object,
    KEY_ROW: uses(Unread, Unread, Unread, Never) = Heap::Key,
    /// A fast arm of `Unary`'s `typeof` index and tests (`staged::value::immediate_typeof_index`): the frame
    /// slot at hole 1 holds a value whose tag decides its `typeof` index, which is the destination's int
    /// (`TYPEOF_INDEX_ROW`, hole 0), or whether it is (`TYPEOF_IS_ROW`) or is not (`TYPEOF_ISNOT_ROW`) the
    /// name at index hole 2, as a boolean. A hit continues at TAKEN; a heap value leaves through EXIT0 into
    /// the helper step after the row.
    TYPEOF_INDEX_ROW: uses(Unread, Unread, Unread, Never) = Heap::TypeofIndex,
    TYPEOF_IS_ROW: uses(Unread, Unread, Unread, Never) = Heap::TypeofIs { negated: false, .. },
    TYPEOF_ISNOT_ROW: uses(Unread, Unread, Unread, Never) = Heap::TypeofIs { negated: true, .. },
    /// A local slot read (`LOAD_CHECKED_ROW`) or an assignment that is no initialization (`STORE_CHECKED_ROW`)
    /// whose binding may be in its temporal dead zone: the slot holds the deleted marker, which leaves through
    /// EXIT0 into the helper step after the row, whose read raises the `ReferenceError`; a hit continues at
    /// TAKEN. The slot number is hole 1 (a read) or hole 2 (the store, whose first operand is the value).
    LOAD_CHECKED_ROW: uses(Dst, Unread, Unread, Never) = Heap::LoadChecked(_),
    STORE_CHECKED_ROW: uses(Unread, Src, Unread, Never) = Heap::StoreChecked(_),
    ]
    [HEAP_END_2 HEAP_TABLE_2:
    /// A fast arm of `Unary`'s `typeof` string (`staged::value::immediate_typeof_string`): the operand (hole 1)
    /// whose tag decides its index, the entry of the table of result strings at hole 2 at that index, to the
    /// destination (hole 0). A hit continues at TAKEN; a heap value leaves through EXIT0 into the helper step.
    TYPEOF_STRING_ROW: uses(Unread, Unread, Unread, Never) = Heap::TypeofString,
    /// The ordinary value at the address in hole 1 (an inlined callee's capture slot, `staged::environment::capture_at`)
    /// into the destination; an internal marker (a dead zone, a shared slot) leaves. A hit continues at NEXT.
    CALLEE_CAPTURE_ROW: uses(Dst, Unread, Unread, Never) = Heap::CalleeCapture,
    // Value 26 is unused: the numbering has always left it free.
    reserve 1,
    /// A fast arm of `Binary`'s `instanceof` of a constructor the code holds (`staged::object::instanceof_by_words`): the
    /// frame slot named by the value operand's offset above the destination's in hole 0 is walked along its prototype chain
    /// for the link above `HELD_LINK_AT` in hole 1, whose low half is the constructor operand's frame offset; hole 2 is the
    /// constructor's identity, which the slot must hold, else the row leaves. A hit writes the boolean to the destination and
    /// continues at TAKEN; a cell the walk does not follow, another constructor or a longer chain leaves through EXIT0 into
    /// the helper step after the row.
    INSTANCEOF_ROW: uses(Unread, Unread, Unread, Never) = Heap::Instanceof,
    /// Leaves through the taken successor unless the VM's active realm (the word at the byte offset of
    /// hole 1 behind the payload's VM) names the global object word of hole 2: the proof once at entry.
    REALM_ROW: uses(Unread, Unread, Unread, Never) = Heap::Realm,
    /// The global object's slot, read into the destination (hole 1: the object's record id, hole 2: the
    /// slot's byte offset), or written from the first operand (hole 0: the record id, hole 2: the byte
    /// offset; the `entry` variant takes the store gate, for a value that may be a heap reference).
    GLOBAL_READ_ROW: uses(Dst, Unread, Unread, Never) = Heap::GlobalRead,
    GLOBAL_STORE_ROW: uses(Unread, Src, Unread, Kept) = Heap::GlobalStore,
    /// The destination takes the tagged word of hole 1.
    CONSTANT_ROW: uses(Dst, Unread, Unread, Never) = Heap::Word,
    /// `AppendArray`'s fast arm: the Number operand (hole 1) appended to the unique writable dense array at the
    /// frame slot of hole 0 where the backing's capacity word proves room (`elements::append_in_room`). A hit
    /// continues at TAKEN, past the helper step laid out after the row (which grows the backing); a miss
    /// leaves through EXIT0 into it.
    APPEND_ROW: uses(Unread, Src, Unread, Never) = Heap::Append,
    /// The property-add rows (`staged::add::site_add`): an object register's or the activation's receiver
    /// (`THIS_ADD_*`) gains the value operand (hole 1) by the transition the holes carry, one row per storage
    /// form the slot admits (`staged::object::Form`). A hit continues at TAKEN, past the helper step laid out
    /// after the row (or the leave a guarded row's miss makes); a miss leaves through EXIT0 into it.
    ADD_PACKED_ROW: uses(Unread, Unread, Unread, Kept) = Heap::Add { form: Form::Packed, this: false },
    ADD_ARENA_ROW: uses(Unread, Unread, Unread, Kept) = Heap::Add { form: Form::Arena, this: false },
    ADD_EITHER_ROW: uses(Unread, Unread, Unread, Kept) = Heap::Add { form: Form::Either, this: false },
    THIS_ADD_PACKED_ROW: uses(Unread, Unread, Unread, Kept) = Heap::Add { form: Form::Packed, this: true },
    THIS_ADD_ARENA_ROW: uses(Unread, Unread, Unread, Kept) = Heap::Add { form: Form::Arena, this: true },
    THIS_ADD_EITHER_ROW: uses(Unread, Unread, Unread, Kept) = Heap::Add { form: Form::Either, this: true },
    /// `ForInNext`'s fast arm (`staged::iteration::for_in_next`): the for-in state in the frame slot of hole 1
    /// yields its next key to the slot of hole 0 where its memo proves the snapshot current. A hit continues
    /// at TAKEN, past the helper step laid out after the row, which also takes the exhausted step's jump; a
    /// miss leaves through EXIT0 into it.
    FOR_IN_ROW: uses(Unread, Unread, Unread, Never) = Heap::ForIn,
    /// `SetFunctionName`'s fast arm (`staged::naming::site_name`): the function in the frame slot whose offset
    /// hole 2 carries (above `NAME_FUNCTION_AT`) takes the name word of hole 1 in the slot hole 2 carries (low
    /// `ADD_SLOT_BITS`), moving from the before-layout to the after-layout hole 0 carries as an add's does, where it
    /// held the empty name. One row per storage form the slot admits. A hit continues at TAKEN, past the helper
    /// step laid out after the row; a miss leaves through EXIT0 into it.
    NAME_EITHER_ROW: uses(Unread, Unread, Unread, Never) = Heap::Name { form: Form::Packed | Form::Either },
    NAME_ARENA_ROW: uses(Unread, Unread, Unread, Never) = Heap::Name { form: Form::Arena },
    /// The inline-cache rows (`ic_rows.rs`): a read of the receiver's own flat slot, a read of the slot its
    /// prototype holds, and a store of a Number into the receiver's own flat slot, whose slot type is `ANY` or
    /// `NUMBER` (`IC_SET_ROW`) or `INT32` (`IC_SET_INT_ROW`).
    IC_GET_ROW: uses(Dst, Unread, Unread, Kept) = Heap::IcGet,
    IC_GET_HOLDER_ROW: uses(Dst, Unread, Unread, Kept),
    IC_SET_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcSet { int: false },
    IC_SET_INT_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcSet { int: true },
    /// A read (`ENV_ROW`) and a store (`ENV_SET_ROW`) of a local slot the running activation's environment window holds
    /// (`staged::environment::environment_slot`), through the storage word the payload keeps
    /// (`JS_STEP_ENV_STORAGE`; one that holds no window reads as sparse storage, which every such row misses at): the slot
    /// at hole 1 and its byte offset at hole 2 as a capture row's, the destination (read) or the value operand (store) at hole 0.
    /// A hit continues at TAKEN, past the helper step laid out after the row; a slot past the run or holding an internal
    /// marker (a dead zone, a slot the environment took) leaves through EXIT0 into it.
    ENV_ROW: uses(Dst, Unread, Unread, Never) = Heap::EnvLocal(_),
    ENV_SET_ROW: uses(Unread, Src, Unread, Never) = Heap::EnvStore(_),
    /// The definition of a slot of that storage (`staged::environment::environment_slot_define`): a binding's first value
    /// (hole 0 the value operand, holes 1 and 2 the slot as `ENV_SET_ROW`'s), over its dead zone or an earlier value, which
    /// `InitializeTdz` (the hole as the operand) and a declaration's store make. A hit continues at TAKEN; a slot past the run
    /// or another environment's shared slot leaves through EXIT0 into the helper step.
    ENV_DEFINE_ROW: uses(Unread, Src, Unread, Never) = Heap::EnvDefine(_),
    ]
    [HEAP_END_3 HEAP_TABLE_3:
    /// The destination (hole 0) takes the word of the global lexical binding at the address in hole 1 (`staged::lexical_read`), or the
    /// first operand (hole 1) is stored to it (address in hole 2) where it is initialized: a binding in its temporal dead zone (the deleted
    /// marker) leaves through EXIT0, so the interpreter's step raises. The address is held under the global layout, which a new declaration
    /// changes.
    LEXICAL_READ_ROW: uses(Dst, Unread, Unread, Never) = Heap::LexicalRead,
    LEXICAL_WRITE_ROW: uses(Unread, Src, Unread, Never) = Heap::LexicalWrite,
    /// A typed array's element of a planned kind (`TypedArrayKind` in discriminant order, `TYPED_KINDS` of them), read into
    /// the destination (`TYPED_INDEX_ROW` + kind) or replaced by the Number operand (`TYPED_STORE_ROW` + kind), through the
    /// view the loop's entry recorded: the entry words' record offset is hole 1 (a read) or hole 0 (a store), the index
    /// operand hole 2 and the store's value hole 1. The entry proved the kind once, so no row tests it; an index past the
    /// count or a value no element converts leaves through EXIT0 with nothing written. `TYPED_LENGTH_ROW` reads the count
    /// the entry words hold (hole 1) as the destination's int32.
    TYPED_INDEX_ROW: uses(Float, Unread, Src, Always) x TYPED_KINDS = Heap::TypedIndex(k) => k,
    TYPED_STORE_ROW: uses(Unread, Src, Src, Always) x TYPED_KINDS = Heap::TypedStore(k) => k,
    TYPED_LENGTH_ROW: uses(Dst, Unread, Unread, Always) = Heap::TypedLength,
    /// The receiver-proof rows (`ic_rows.rs`, `base_region::proofs`): the proof that the object register is of the
    /// entry's exact shape with flat values, and the guardless read and stores of a receiver every path proved so.
    IC_PROOF_ROW: uses(Unread, Unread, Unread, Never) = Heap::IcProof,
    IC_KNOWN_GET_ROW: uses(Dst, Unread, Unread, Kept) = Heap::IcKnownGet,
    IC_KNOWN_SET_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcKnownSet { int: false, admitted: false },
    IC_KNOWN_SET_INT_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcKnownSet { int: true, admitted: false },
    /// `IC_KNOWN_SET_ROW` of a store the region's entry admitted (`base_region::proofs`): no run-level stores test, the entry
    /// test (`STORES_ROW`) having held and no call since.
    IC_KNOWN_SET_ADMITTED_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcKnownSet { int: false, admitted: true },
    IC_KNOWN_SET_INT_ADMITTED_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcKnownSet { int: true, admitted: true },
    /// `IC_KNOWN_GET_ROW` / `IC_KNOWN_SET_ROW` of a receiver whose entry proved its exact shape and flat form (`SHAPE_FLAT_ROW`,
    /// `base_region::proofs`): the slot at its byte offset in the arena run, with no compare of the auxiliary word. The
    /// flat set's holes are the admitted row's (`ADMITTED`) or the stores test's (`IC_KNOWN_SET_FLAT_ROW`) plus the offset.
    IC_KNOWN_GET_FLAT_ROW: uses(Dst, Unread, Unread, Kept) = Heap::IcKnownGetFlat,
    IC_KNOWN_SET_FLAT_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcKnownSetFlat { int: false, admitted: false },
    IC_KNOWN_SET_INT_FLAT_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcKnownSetFlat { int: true, admitted: false },
    IC_KNOWN_SET_FLAT_ADMITTED_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcKnownSetFlat { int: false, admitted: true },
    IC_KNOWN_SET_INT_FLAT_ADMITTED_ROW: uses(Unread, Src, Unread, Kept) = Heap::IcKnownSetFlat { int: true, admitted: true },
    /// A declared parameter's entry test (`base_region::link::entry_rows`): the frame slot at hole 0 holds a word
    /// whose language its tag alone decides, in the mask of hole 1 (`shared_words::language`), where the mask
    /// is the declared set. A hit continues at NEXT; a miss leaves through EXIT0 with nothing written.
    DECLARED_ROW: uses(Unread, Unread, Unread, Never) = Heap::Declared,
    /// A captured declaration's entry test (`base_region::link::entry_rows`): the depth-0 capture at the slot of hole 0
    /// (byte offset in hole 1) holds exactly the word of hole 2 (the callee the body calls by identity). A hit continues at NEXT; a miss leaves
    /// through EXIT0 with nothing written.
    CAPTURE_IDENTITY_ROW: uses(Unread, Unread, Unread, Never) = Heap::CaptureIdentity,
    /// A class-typed parameter's entry test (`base_region::link::entry_rows`): the frame slot at hole 0 holds a direct
    /// object of the exact shape in hole 1's low half whose prototype link is its high half (`HELD_LINK_AT`), the record
    /// found in the payload's object record table (`guard::prototype`). A hit continues at NEXT; a miss leaves through
    /// EXIT0 with nothing written.
    PROTOTYPE_ROW: uses(Unread, Unread, Unread, Never) = Heap::Prototype,
    /// A loop-invariant receiver's entry test (`base_region::link::entry_rows`): the frame slot at hole 0 holds a direct
    /// object of the exact shape in hole 1 (its record found in the payload's object record table, `guard::shape`). A hit
    /// continues at NEXT; a miss leaves through EXIT0 with nothing written.
    SHAPE_ROW: uses(Unread, Unread, Unread, Never) = Heap::Shape,
    /// `SHAPE_ROW` of a receiver whose flat form the entry checks too: the auxiliary word is the shape in hole 1 with no
    /// storage-form bit (`guard::shape` at `Form::Arena`), so its values are the arena run's.
    SHAPE_FLAT_ROW: uses(Unread, Unread, Unread, Never) = Heap::ShapeFlat,
    /// A region's entry test that the run stores specialized (`JS_STEP_STORES_SPECIALIZED`, `slot_store::store_admitted`):
    /// a hit continues at NEXT; a miss leaves through EXIT0 with nothing written.
    STORES_ROW: uses(Unread, Unread, Unread, Never) = Heap::Stores,
    /// `String.prototype.charCodeAt` of a flat string (hole 1: the receiver's frame slot) at an int32 index in range (hole 2):
    /// the code unit, an int32 into the destination (hole 0). Any other receiver, index or form leaves through EXIT0 with
    /// nothing written, to the call's own step.
    CHAR_CODE_AT_ROW: uses(Dst, Unread, Src, Never) = Heap::CharCodeAt,
    /// `parseInt` of a flat string (hole 1: the frame slot) in radix 10 (`staged::parse_int::decimal_int32_row`): the int32
    /// into the destination (hole 0). Any other text leaves through EXIT0 with nothing written.
    PARSE_INT_ROW: uses(Dst, Unread, Unread, Never) = Heap::ParseInt,
    /// The staged rows (`staged_rows.rs`): a Number operation the planner's operator tables do not name, a relation or
    /// an equality in value form, `~`, `void`, `InitializeTdz` of a frame local and `RequireObjectCoercible`. Each is a
    /// fast arm over register operands, its destination a frame slot; `STAGED_FIRST..=STAGED_LAST` is the run
    /// `heap_rows::run` routes to `staged_rows::run`.
    STAGED_REMAINDER: uses(Unread, Src, Src, Never),
    STAGED_RELVAL_LT: uses(Unread, Src, Src, Never),
    STAGED_RELVAL_LE: uses(Unread, Src, Src, Never),
    STAGED_RELVAL_GT: uses(Unread, Src, Src, Never),
    STAGED_RELVAL_GE: uses(Unread, Src, Src, Never),
    STAGED_RELVAL_EQ: uses(Unread, Src, Src, Never),
    ]
    [HEAP_END_4 HEAP_TABLE_4:
    STAGED_RELVAL_NE: uses(Unread, Src, Src, Never),
    STAGED_BITWISE_NOT: uses(Unread, Src, Unread, Never),
    STAGED_INIT_LOCAL: uses(Unread, Unread, Unread, Never),
    STAGED_COERCIBLE: uses(Unread, Src, Unread, Never),
    STAGED_VOID: uses(Unread, Unread, Unread, Never),
    STAGED_LOOSE_EQ: uses(Unread, Src, Src, Never),
    STAGED_LOOSE_NE: uses(Unread, Src, Src, Never),
    STAGED_STRICT_EQ: uses(Unread, Src, Src, Never),
    STAGED_STRICT_NE: uses(Unread, Src, Src, Never),
    /// An empty object (`EMPTY_OBJECT_ROW`) or zero-length array (`EMPTY_ARRAY_ROW`) bumped from its kind's record cursor
    /// and template (`heap_rows::empty_row`): the destination (hole 0) takes the new reference. A spent run, a missing
    /// template or a destination that cannot hold a reference leaves through EXIT0 with nothing written.
    EMPTY_OBJECT_ROW: uses(Dst, Unread, Unread, Never),
    EMPTY_ARRAY_ROW: uses(Dst, Unread, Unread, Never),
    ]
    }
    number {
    MOVE,
    COPY,
    ADD,
    SUB,
    MUL,
    DIV,
    AND,
    OR,
    XOR,
    SHL,
    SHR,
    USHR,
    LT,
    LE,
    GT,
    GE,
    EQ,
    NE,
    JUMP,
    JUMP_POLL,
    GUARD,
    /// `GUARD` of a pinned double with its tag test left out: the destination takes the frame slot's word as a binary64
    /// (an int word widened). A proven-arguments entry runs it where the caller proved the argument a Number
    /// (`numeric::unbox_row`).
    UNBOX,
    /// `GUARD` of a pinned int32 with its tag test left out: the destination takes the frame slot's low word. A
    /// proven-arguments entry runs it where the caller proved the argument an int32 (`numeric::unbox_int_row`).
    UNBOX_INT,
    TAG_INIT,
    SPILL,
    JUMP_WATCH,
    /// Polls the interrupt flag and continues: the guard a conditional branch back passes first.
    POLL,
    /// Branches to the taken successor when the first operand is truthy, or falsy (`JUMP_FALSE`).
    JUMP_TRUE,
    JUMP_FALSE,
    /// The destination takes the logical negation of the first operand's truthiness, or the Number
    /// negation of the first operand.
    NOT,
    NEG,
    /// `%`: the staged Number remainder (`NumberOperation::Remainder`).
    REM,
    /// The Math functions of one Number: the staged `NumberOperation::Math`, a double, the one definition
    /// the native runs (`staged::math::float_unary`): `sqrtsd`, `roundsd` (x86-64-v2 and up), a sign mask in
    /// a register, and `Math.round`'s composition of them, its constants immediates (`machine::constant`).
    SQRT,
    FLOOR,
    CEIL,
    TRUNC,
    ABS,
    ROUND,
    /// `Math.fround` (the nearest binary32, as a double) and `Math.sign` of one Number, in the same staged native call.
    FROUND,
    SIGN,
    /// `Math.min`, `Math.max` of two Numbers (a double: `staged::math::float_extreme`) and `Math.imul` of
    /// two int32s (`staged::math::int_multiply`).
    MIN,
    MAX,
    IMUL,
    /// `Math.clz32` of one Number: the leading zero bits of its ToUint32 (`staged::math::leading_zeros`), an int32.
    CLZ32,
    /// `switch`'s case slot (`NumberOperation::IntCase`): the slot of the first operand among the labels from the
    /// second's immediate to the maximum at hole 3, an int32 into the destination. Never leaves.
    INT_CASE,
    /// int32 `+` and `-` the planner proved exact (a range fact of the operands): the staged int32 step
    /// with `Overflow::Impossible`, so no overflow test. Int operands only; anything else leaves through exit 0.
    ADD_NO_OVERFLOW,
    SUB_NO_OVERFLOW,
    /// The jump of `if (a & b)`: branches to the taken successor unless the int32 of `a & b` is nonzero
    /// (`Comparison::Bitwise(And)`, the staged compare step's own bitwise test).
    BIT_TEST,
    /// The jump of `a == null` or `a == undefined` (loose, `Comparison::Nullish`); `NULLISH_NE` is `a != null`.
    /// The tested operand's tag decides it; the row has no second operand.
    NULLISH_EQ,
    NULLISH_NE,
    /// The jump of `a === true` and of `a === false` (`Comparison::Boolean`): the tested word's identity decides it.
    BOOL_TRUE,
    BOOL_FALSE,
    }
        }
    };
}
