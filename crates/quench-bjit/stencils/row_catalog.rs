//! The row catalog: every Number row kind and heap row the b-jit library names, once, in the order
//! their codes are numbered. `codes.rs` includes it by path with `#[macro_use]`, so its `pub const`s
//! are the codes; `library.rs` reads each heap row's operand slots and entry mode from `HEAP_TABLES`.
//!
//! A heap row is one line: `NAME: uses(dst, a, b, entry),` where `dst`, `a`, `b` are the `Slot`s its
//! operands range over and `entry` is its `Entry` mode, and `x COUNT` makes a family of `COUNT` codes
//! that share the line (a typed kind family). `reserve N` skips N codes no row holds. Rows are listed in
//! chunks, each a `[END TABLE: ...]`, because the positional values chain through the chunks and a
//! single macro expansion over every row would nest deeper than the default recursion limit.
//! Adding a heap row: one line here, and a `Heap` variant with its arm in `Heap::code` in `runtime base_rows.rs`
//! (the test there checks that every variant's code is one of these rows; it must list the new variant too).

#![allow(dead_code)]

use super::TYPED_KINDS;

/// How a heap row reads its entry flag: it ignores it, keeps the site's, or is always an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Never,
    Kept,
    Always,
}

/// The places one operand of a heap row ranges over: a row that does not read the operand fixes it to `MEM`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Unread,
    /// Any destination: every int place and every double register.
    Dst,
    /// A typed element read's destination: a double register or `MEM`.
    Float,
    /// Any operand: every int place, the constant and immediate, and every double register.
    Src,
}

/// The places a heap row reads: its entry flag and the places each operand ranges over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uses {
    pub entry: Entry,
    pub dst: Slot,
    pub a: Slot,
    pub b: Slot,
}

/// Numbers the listed codes consecutively from 0, in the listed order: each `NAME` becomes
/// `pub const NAME: u8` holding the next value, its doc comments attached, and `reserve N` skips
/// `N` values that no code holds. The count after the last value is the last argument's name
/// (`ROW_CODE_COUNT`, one past the highest code). A new code is a new line, never a typed number.
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
    ($count:ident: $($list:tt)*) => {
        numbered_codes!(@emit $count, 0; $($list)*);
    };
}

/// One chunk of heap rows: `$start` is the code the chunk's first row takes, `$end` the first code after
/// the chunk, `$table` the chunk's `(first code, count, uses)` entries.
macro_rules! heap_chunk {
    ($end:ident, $table:ident, $start:expr; $($body:tt)*) => {
        heap_chunk!(@go $end, $table, $start, []; $($body)*);
    };
    (@go $end:ident, $table:ident, $base:expr, [$($acc:tt)*];) => {
        const $end: u8 = $base;
        const $table: &[(u8, u8, Uses)] = &[$($acc)*];
    };
    (@go $end:ident, $table:ident, $base:expr, [$($acc:tt)*]; reserve $n:expr $(, $($rest:tt)*)?) => {
        heap_chunk!(@go $end, $table, $base + $n, [$($acc)*]; $($($rest)*)?);
    };
    (@go $end:ident, $table:ident, $base:expr, [$($acc:tt)*];
        $(#[$m:meta])* $name:ident : unlisted $(, $($rest:tt)*)?) => {
        $(#[$m])* pub const $name: u8 = $base;
        heap_chunk!(@go $end, $table, $base + 1, [$($acc)*]; $($($rest)*)?);
    };
    (@go $end:ident, $table:ident, $base:expr, [$($acc:tt)*];
        $(#[$m:meta])* $name:ident : uses($dst:ident, $a:ident, $b:ident, $entry:ident) $(x $n:expr)? $(, $($rest:tt)*)?) => {
        $(#[$m])* pub const $name: u8 = $base;
        heap_chunk!(@go $end, $table, $base + (1 $(+ ($n) - 1)?),
            [$($acc)* ($base, 1 $(+ ($n) - 1)?, Uses { entry: Entry::$entry, dst: Slot::$dst, a: Slot::$a, b: Slot::$b }),];
            $($($rest)*)?);
    };
}

/// The whole catalog: the heap chunks in order (each chained from the one before), then the Number kinds.
macro_rules! row_catalog {
    (heap { $($chunk:tt)* } number { $($kinds:tt)* }) => {
        row_catalog!(@heap 0, []; $($chunk)*);
        numbered_codes! { KIND_CODE_COUNT: $($kinds)* }
    };
    (@heap $prev:expr, [$($tables:ident,)*];) => {
        pub(crate) const HEAP_TABLES: &[&[(u8, u8, Uses)]] = &[$($tables),*];
        pub const ROW_CODE_COUNT: u8 = $prev;
    };
    (@heap $prev:expr, [$($tables:ident,)*]; [$end:ident $table:ident: $($body:tt)*] $($rest:tt)*) => {
        heap_chunk!($end, $table, $prev; $($body)*);
        row_catalog!(@heap $end, [$($tables,)* $table,]; $($rest)*);
    };
}

row_catalog! {
    heap {
    [HEAP_END_1 HEAP_TABLE_1:
    CAPTURE_ROW: uses(Dst, Unread, Unread, Never),
    INDEX_ROW: uses(Dst, Unread, Src, Kept),
    STORE_ROW: uses(Unread, Src, Src, Kept),
    FIELD_ROW: uses(Dst, Unread, Unread, Never),
    ARRAY_ROW: uses(Unread, Unread, Unread, Never),
    SITE_ROW: uses(Dst, Unread, Unread, Kept),
    /// The running activation's `this`, a local slot read, a local slot written.
    THIS_ROW: uses(Dst, Unread, Unread, Never),
    LOAD_LOCAL_ROW: uses(Dst, Unread, Unread, Never),
    STORE_LOCAL_ROW: uses(Unread, Src, Unread, Never),
    /// The frame slot at hole 1 holds exactly the word at hole 2: the proof a call's callee is the one
    /// a site was stitched for.
    IDENTITY_ROW: uses(Unread, Unread, Unread, Never),
    LENGTH_ROW: uses(Dst, Unread, Unread, Never),
    CAPTURE_SET_ROW: uses(Unread, Src, Unread, Never),
    SITE_STORE_ROW: uses(Unread, Src, Unread, Kept),
    /// The site rows of the activation's receiver (`this`), which the payload holds.
    THIS_SITE_ROW: uses(Dst, Unread, Unread, Kept),
    THIS_STORE_ROW: uses(Unread, Src, Unread, Kept),
    /// A prototype-held field folded to the value its holder held when the code was compiled, over the
    /// receiver (shape and prototype link) it stands on; the receiver's variant (`entry`) takes it from the payload.
    HELD_ROW: uses(Dst, Unread, Unread, Kept),
    /// Leaves unless the invalidation word at the address in hole 1 is zero.
    LIVE_ROW: uses(Unread, Unread, Unread, Never),
    /// A fast arm of an instruction whose full operation a helper step makes (`staged::iteration`): the
    /// frame slot at hole 1 holds a direct object or array record (`RequireIteratorResult`), or a direct
    /// string, which is copied to the slot at hole 0 (`ToPropertyKey`). A hit continues at TAKEN, past the
    /// helper step laid out after the row; a miss leaves through EXIT0 into it.
    OBJECT_ROW: uses(Unread, Unread, Unread, Never),
    KEY_ROW: uses(Unread, Unread, Unread, Never),
    /// A fast arm of `Unary`'s `typeof` index and tests (`staged::value::immediate_typeof_index`): the frame
    /// slot at hole 1 holds a value whose tag decides its `typeof` index, which is the destination's int
    /// (`TYPEOF_INDEX_ROW`, hole 0), or whether it is (`TYPEOF_IS_ROW`) or is not (`TYPEOF_ISNOT_ROW`) the
    /// name at index hole 2, as a boolean. A hit continues at TAKEN; a heap value leaves through EXIT0 into
    /// the helper step after the row.
    TYPEOF_INDEX_ROW: uses(Unread, Unread, Unread, Never),
    TYPEOF_IS_ROW: uses(Unread, Unread, Unread, Never),
    TYPEOF_ISNOT_ROW: uses(Unread, Unread, Unread, Never),
    /// A local slot read (`LOAD_CHECKED_ROW`) or an assignment that is no initialization (`STORE_CHECKED_ROW`)
    /// whose binding may be in its temporal dead zone: the slot holds the deleted marker, which leaves through
    /// EXIT0 into the helper step after the row, whose read raises the `ReferenceError`; a hit continues at
    /// TAKEN. The slot number is hole 1 (a read) or hole 2 (the store, whose first operand is the value).
    LOAD_CHECKED_ROW: uses(Dst, Unread, Unread, Never),
    STORE_CHECKED_ROW: uses(Unread, Src, Unread, Never),
    ]
    [HEAP_END_2 HEAP_TABLE_2:
    /// A fast arm of `Unary`'s `typeof` string (`staged::value::immediate_typeof_string`): the operand (hole 1)
    /// whose tag decides its index, the entry of the table of result strings at hole 2 at that index, to the
    /// destination (hole 0). A hit continues at TAKEN; a heap value leaves through EXIT0 into the helper step.
    TYPEOF_STRING_ROW: uses(Unread, Unread, Unread, Never),
    /// The ordinary value at the address in hole 1 (an inlined callee's capture slot, `staged::environment::capture_at`)
    /// into the destination; an internal marker (a dead zone, a shared slot) leaves. A hit continues at NEXT.
    CALLEE_CAPTURE_ROW: uses(Dst, Unread, Unread, Never),
    // Value 26 is unused: the numbering has always left it free.
    reserve 1,
    /// A fast arm of `Binary`'s `instanceof` of a constructor the code holds (`staged::object::instanceof_by_words`): the
    /// frame slot named by the value operand's offset above the destination's in hole 0 is walked along its prototype chain
    /// for the link above `HELD_LINK_AT` in hole 1, whose low half is the constructor operand's frame offset; hole 2 is the
    /// constructor's identity, which the slot must hold, else the row leaves. A hit writes the boolean to the destination and
    /// continues at TAKEN; a cell the walk does not follow, another constructor or a longer chain leaves through EXIT0 into
    /// the helper step after the row.
    INSTANCEOF_ROW: uses(Unread, Unread, Unread, Never),
    /// Leaves through the taken successor unless the VM's active realm (the word at the byte offset of
    /// hole 1 behind the payload's VM) names the global object word of hole 2: the proof once at entry.
    REALM_ROW: uses(Unread, Unread, Unread, Never),
    /// The global object's slot, read into the destination (hole 1: the object's record id, hole 2: the
    /// slot's byte offset), or written from the first operand (hole 0: the record id, hole 2: the byte
    /// offset; the `entry` variant takes the store gate, for a value that may be a heap reference).
    GLOBAL_READ_ROW: uses(Dst, Unread, Unread, Never),
    GLOBAL_STORE_ROW: uses(Unread, Src, Unread, Kept),
    /// The destination takes the tagged word of hole 1.
    CONSTANT_ROW: uses(Dst, Unread, Unread, Never),
    /// `AppendArray`'s fast arm: the Number operand (hole 1) appended to the unique writable dense array at the
    /// frame slot of hole 0 where the backing's capacity word proves room (`elements::append_in_room`). A hit
    /// continues at TAKEN, past the helper step laid out after the row (which grows the backing); a miss
    /// leaves through EXIT0 into it.
    APPEND_ROW: uses(Unread, Src, Unread, Never),
    /// The property-add rows (`staged::add::site_add`): an object register's or the activation's receiver
    /// (`THIS_ADD_*`) gains the value operand (hole 1) by the transition the holes carry, one row per storage
    /// form the slot admits (`staged::object::Form`). A hit continues at TAKEN, past the helper step laid out
    /// after the row (or the leave a guarded row's miss makes); a miss leaves through EXIT0 into it.
    ADD_PACKED_ROW: uses(Unread, Unread, Unread, Kept),
    ADD_ARENA_ROW: uses(Unread, Unread, Unread, Kept),
    ADD_EITHER_ROW: uses(Unread, Unread, Unread, Kept),
    THIS_ADD_PACKED_ROW: uses(Unread, Unread, Unread, Kept),
    THIS_ADD_ARENA_ROW: uses(Unread, Unread, Unread, Kept),
    THIS_ADD_EITHER_ROW: uses(Unread, Unread, Unread, Kept),
    /// `ForInNext`'s fast arm (`staged::iteration::for_in_next`): the for-in state in the frame slot of hole 1
    /// yields its next key to the slot of hole 0 where its memo proves the snapshot current. A hit continues
    /// at TAKEN, past the helper step laid out after the row, which also takes the exhausted step's jump; a
    /// miss leaves through EXIT0 into it.
    FOR_IN_ROW: uses(Unread, Unread, Unread, Never),
    /// `SetFunctionName`'s fast arm (`staged::naming::site_name`): the function in the frame slot whose offset
    /// hole 2 carries (above `NAME_FUNCTION_AT`) takes the name word of hole 1 in the slot hole 2 carries (low
    /// `ADD_SLOT_BITS`), moving from the before-layout to the after-layout hole 0 carries as an add's does, where it
    /// held the empty name. One row per storage form the slot admits. A hit continues at TAKEN, past the helper
    /// step laid out after the row; a miss leaves through EXIT0 into it.
    NAME_EITHER_ROW: uses(Unread, Unread, Unread, Never),
    NAME_ARENA_ROW: uses(Unread, Unread, Unread, Never),
    /// The inline-cache rows (`ic_rows.rs`): a read of the receiver's own flat slot, a read of the slot its
    /// prototype holds, and a store of a Number into the receiver's own flat slot, whose slot type is `ANY` or
    /// `NUMBER` (`IC_SET_ROW`) or `INT32` (`IC_SET_INT_ROW`).
    IC_GET_ROW: uses(Dst, Unread, Unread, Kept),
    IC_GET_HOLDER_ROW: uses(Dst, Unread, Unread, Kept),
    IC_SET_ROW: uses(Unread, Src, Unread, Kept),
    IC_SET_INT_ROW: uses(Unread, Src, Unread, Kept),
    /// A read (`ENV_ROW`) and a store (`ENV_SET_ROW`) of a local slot the running activation's environment window holds
    /// (`staged::environment::environment_slot`), through the storage word the payload keeps
    /// (`JS_STEP_ENV_STORAGE`; one that holds no window reads as sparse storage, which every such row misses at): the slot
    /// at hole 1 and its byte offset at hole 2 as a capture row's, the destination (read) or the value operand (store) at hole 0.
    /// A hit continues at TAKEN, past the helper step laid out after the row; a slot past the run or holding an internal
    /// marker (a dead zone, a slot the environment took) leaves through EXIT0 into it.
    ENV_ROW: uses(Dst, Unread, Unread, Never),
    ENV_SET_ROW: uses(Unread, Src, Unread, Never),
    /// The definition of a slot of that storage (`staged::environment::environment_slot_define`): a binding's first value
    /// (hole 0 the value operand, holes 1 and 2 the slot as `ENV_SET_ROW`'s), over its dead zone or an earlier value, which
    /// `InitializeTdz` (the hole as the operand) and a declaration's store make. A hit continues at TAKEN; a slot past the run
    /// or another environment's shared slot leaves through EXIT0 into the helper step.
    ENV_DEFINE_ROW: uses(Unread, Src, Unread, Never),
    ]
    [HEAP_END_3 HEAP_TABLE_3:
    /// The destination (hole 0) takes the word of the global lexical binding at the address in hole 1 (`staged::lexical_read`), or the
    /// first operand (hole 1) is stored to it (address in hole 2) where it is initialized: a binding in its temporal dead zone (the deleted
    /// marker) leaves through EXIT0, so the interpreter's step raises. The address is held under the global layout, which a new declaration
    /// changes.
    LEXICAL_READ_ROW: uses(Dst, Unread, Unread, Never),
    LEXICAL_WRITE_ROW: uses(Unread, Src, Unread, Never),
    /// A typed array's element of a planned kind (`TypedArrayKind` in discriminant order, `TYPED_KINDS` of them), read into
    /// the destination (`TYPED_INDEX_ROW` + kind) or replaced by the Number operand (`TYPED_STORE_ROW` + kind), through the
    /// view the loop's entry recorded: the entry words' record offset is hole 1 (a read) or hole 0 (a store), the index
    /// operand hole 2 and the store's value hole 1. The entry proved the kind once, so no row tests it; an index past the
    /// count or a value no element converts leaves through EXIT0 with nothing written. `TYPED_LENGTH_ROW` reads the count
    /// the entry words hold (hole 1) as the destination's int32.
    TYPED_INDEX_ROW: uses(Float, Unread, Src, Always) x TYPED_KINDS,
    TYPED_STORE_ROW: uses(Unread, Src, Src, Always) x TYPED_KINDS,
    TYPED_LENGTH_ROW: uses(Dst, Unread, Unread, Always),
    /// The receiver-proof rows (`ic_rows.rs`, `base_region::proofs`): the proof that the object register is of the
    /// entry's exact shape with flat values, and the guardless read and stores of a receiver every path proved so.
    IC_PROOF_ROW: uses(Unread, Unread, Unread, Never),
    IC_KNOWN_GET_ROW: uses(Dst, Unread, Unread, Kept),
    IC_KNOWN_SET_ROW: uses(Unread, Src, Unread, Kept),
    IC_KNOWN_SET_INT_ROW: uses(Unread, Src, Unread, Kept),
    /// `IC_KNOWN_SET_ROW` of a store the region's entry admitted (`base_region::proofs`): no run-level stores test, the entry
    /// test (`STORES_ROW`) having held and no call since.
    IC_KNOWN_SET_ADMITTED_ROW: uses(Unread, Src, Unread, Kept),
    IC_KNOWN_SET_INT_ADMITTED_ROW: uses(Unread, Src, Unread, Kept),
    /// A declared parameter's entry test (`base_region::link::entry_rows`): the frame slot at hole 0 holds a word
    /// whose language its tag alone decides, in the mask of hole 1 (`shared_words::language`), where the mask
    /// is the declared set. A hit continues at NEXT; a miss leaves through EXIT0 with nothing written.
    DECLARED_ROW: uses(Unread, Unread, Unread, Never),
    /// A captured declaration's entry test (`base_region::link::entry_rows`): the depth-0 capture at the slot of hole 0
    /// holds exactly the word of hole 1 (the callee the body calls by identity). A hit continues at NEXT; a miss leaves
    /// through EXIT0 with nothing written.
    CAPTURE_IDENTITY_ROW: uses(Unread, Unread, Unread, Never),
    /// A class-typed parameter's entry test (`base_region::link::entry_rows`): the frame slot at hole 0 holds a direct
    /// object of the exact shape in hole 1's low half whose prototype link is its high half (`HELD_LINK_AT`), the record
    /// found in the payload's object record table (`guard::prototype`). A hit continues at NEXT; a miss leaves through
    /// EXIT0 with nothing written.
    PROTOTYPE_ROW: uses(Unread, Unread, Unread, Never),
    /// A loop-invariant receiver's entry test (`base_region::link::entry_rows`): the frame slot at hole 0 holds a direct
    /// object of the exact shape in hole 1 (its record found in the payload's object record table, `guard::shape`). A hit
    /// continues at NEXT; a miss leaves through EXIT0 with nothing written.
    SHAPE_ROW: uses(Unread, Unread, Unread, Never),
    /// A region's entry test that the run stores specialized (`JS_STEP_STORES_SPECIALIZED`, `slot_store::store_admitted`):
    /// a hit continues at NEXT; a miss leaves through EXIT0 with nothing written.
    STORES_ROW: uses(Unread, Unread, Unread, Never),
    /// `String.prototype.charCodeAt` of a flat string (hole 1: the receiver's frame slot) at an int32 index in range (hole 2):
    /// the code unit, an int32 into the destination (hole 0). Any other receiver, index or form leaves through EXIT0 with
    /// nothing written, to the call's own step.
    CHAR_CODE_AT_ROW: uses(Dst, Unread, Src, Never),
    /// `parseInt` of a flat string (hole 1: the frame slot) in radix 10 (`staged::parse_int::decimal_int32`): the int32
    /// into the destination (hole 0). Any other text leaves through EXIT0 with nothing written.
    PARSE_INT_ROW: uses(Dst, Unread, Unread, Never),
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
    /// `Math.min`, `Math.max` of two Numbers (a double: `staged::math::float_extreme`) and `Math.imul` of
    /// two int32s (`staged::math::int_multiply`).
    MIN,
    MAX,
    IMUL,
    /// `Math.clz32` of one Number: the leading zero bits of its ToUint32 (`staged::math::leading_zeros`), an int32.
    CLZ32,
    /// int32 `+` and `-` the planner proved exact (a range fact of the operands): the staged int32 step
    /// with `Overflow::Impossible`, so no overflow test. Int operands only; anything else leaves through exit 0.
    ADD_NO_OVERFLOW,
    SUB_NO_OVERFLOW,
    }
}
