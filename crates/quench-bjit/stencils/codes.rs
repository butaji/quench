//! The codes a Number or heap row is named by: the places of its operands, its kind and its heap
//! operation. The one definition `numeric.rs` (the semantics), `library.rs` (which rows exist) and
//! the runtime (which row it plans) all read.

#![allow(dead_code)]

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

pub const MEM: u8 = 0;
pub const MEMINT: u8 = 1;
pub const CONST: u8 = 2;
pub const IMM: u8 = 3;
pub const PIN: u8 = 4;
pub const PINF: u8 = 8;
/// Pinned int32 registers, then pinned double registers.
pub const INT_PINS: u8 = 4;
pub const DOUBLE_PINS: u8 = 8;
pub const LAST_INT_PIN: u8 = PIN + INT_PINS - 1;

numbered_codes! {
    KIND_CODE_COUNT:
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
}

/// The first and last of the Math rows of one Number.
pub const MATH_FIRST: u8 = SQRT;
pub const MATH_LAST: u8 = ROUND;

numbered_codes! {
    ROW_CODE_COUNT:
    CAPTURE_ROW,
    INDEX_ROW,
    STORE_ROW,
    FIELD_ROW,
    ARRAY_ROW,
    SITE_ROW,
    /// The running activation's `this`, a local slot read, a local slot written.
    THIS_ROW,
    LOAD_LOCAL_ROW,
    STORE_LOCAL_ROW,
    /// The frame slot at hole 1 holds exactly the word at hole 2: the proof a call's callee is the one
    /// a site was stitched for.
    IDENTITY_ROW,
    LENGTH_ROW,
    CAPTURE_SET_ROW,
    SITE_STORE_ROW,
    /// The site rows of the activation's receiver (`this`), which the payload holds.
    THIS_SITE_ROW,
    THIS_STORE_ROW,
    /// A prototype-held field folded to the value its holder held when the code was compiled, over the
    /// receiver (shape and prototype link) it stands on; the receiver's variant (`entry`) takes it from the payload.
    HELD_ROW,
    /// Leaves unless the invalidation word at the address in hole 1 is zero.
    LIVE_ROW,
    /// A fast arm of an instruction whose full operation a helper step makes (`staged::iteration`): the
    /// frame slot at hole 1 holds a direct object or array record (`RequireIteratorResult`), or a direct
    /// string, which is copied to the slot at hole 0 (`ToPropertyKey`). A hit continues at TAKEN, past the
    /// helper step laid out after the row; a miss leaves through EXIT0 into it.
    OBJECT_ROW,
    KEY_ROW,
    /// A fast arm of `Unary`'s `typeof` index and tests (`staged::value::immediate_typeof_index`): the frame
    /// slot at hole 1 holds a value whose tag decides its `typeof` index, which is the destination's int
    /// (`TYPEOF_INDEX_ROW`, hole 0), or whether it is (`TYPEOF_IS_ROW`) or is not (`TYPEOF_ISNOT_ROW`) the
    /// name at index hole 2, as a boolean. A hit continues at TAKEN; a heap value leaves through EXIT0 into
    /// the helper step after the row.
    TYPEOF_INDEX_ROW,
    TYPEOF_IS_ROW,
    TYPEOF_ISNOT_ROW,
    /// A local slot read (`LOAD_CHECKED_ROW`) or an assignment that is no initialization (`STORE_CHECKED_ROW`)
    /// whose binding may be in its temporal dead zone: the slot holds the deleted marker, which leaves through
    /// EXIT0 into the helper step after the row, whose read raises the `ReferenceError`; a hit continues at
    /// TAKEN. The slot number is hole 1 (a read) or hole 2 (the store, whose first operand is the value).
    LOAD_CHECKED_ROW,
    STORE_CHECKED_ROW,
    /// A fast arm of `Unary`'s `typeof` string (`staged::value::immediate_typeof_string`): the operand (hole 1)
    /// whose tag decides its index, the entry of the table of result strings at hole 2 at that index, to the
    /// destination (hole 0). A hit continues at TAKEN; a heap value leaves through EXIT0 into the helper step.
    TYPEOF_STRING_ROW,
    /// The ordinary value at the address in hole 1 (an inlined callee's capture slot, `staged::environment::capture_at`)
    /// into the destination; an internal marker (a dead zone, a shared slot) leaves. A hit continues at NEXT.
    CALLEE_CAPTURE_ROW,
    // Value 26 is unused: the numbering has always left it free.
    reserve 1,
    /// A fast arm of `Binary`'s `instanceof` of a constructor the code holds (`staged::object::instanceof_by_words`): the
    /// frame slot named by the value operand's offset above the destination's in hole 0 is walked along its prototype chain
    /// for the link above `HELD_LINK_AT` in hole 1, whose low half is the constructor operand's frame offset; hole 2 is the
    /// constructor's identity, which the slot must hold, else the row leaves. A hit writes the boolean to the destination and
    /// continues at TAKEN; a cell the walk does not follow, another constructor or a longer chain leaves through EXIT0 into
    /// the helper step after the row.
    INSTANCEOF_ROW,
    /// Leaves through the taken successor unless the VM's active realm (the word at the byte offset of
    /// hole 1 behind the payload's VM) names the global object word of hole 2: the proof once at entry.
    REALM_ROW,
    /// The global object's slot, read into the destination (hole 1: the object's record id, hole 2: the
    /// slot's byte offset), or written from the first operand (hole 0: the record id, hole 2: the byte
    /// offset; the `entry` variant takes the store gate, for a value that may be a heap reference).
    GLOBAL_READ_ROW,
    GLOBAL_STORE_ROW,
    /// The destination takes the tagged word of hole 1.
    CONSTANT_ROW,
    /// `AppendArray`'s fast arm: the Number operand (hole 1) appended to the unique writable dense array at the
    /// frame slot of hole 0 where the backing's capacity word proves room (`elements::append_in_room`). A hit
    /// continues at TAKEN, past the helper step laid out after the row (which grows the backing); a miss
    /// leaves through EXIT0 into it.
    APPEND_ROW,
    /// The property-add rows (`staged::add::site_add`): an object register's or the activation's receiver
    /// (`THIS_ADD_*`) gains the value operand (hole 1) by the transition the holes carry, one row per storage
    /// form the slot admits (`staged::object::Form`). A hit continues at TAKEN, past the helper step laid out
    /// after the row (or the leave a guarded row's miss makes); a miss leaves through EXIT0 into it.
    ADD_PACKED_ROW,
    ADD_ARENA_ROW,
    ADD_EITHER_ROW,
    THIS_ADD_PACKED_ROW,
    THIS_ADD_ARENA_ROW,
    THIS_ADD_EITHER_ROW,
    /// `ForInNext`'s fast arm (`staged::iteration::for_in_next`): the for-in state in the frame slot of hole 1
    /// yields its next key to the slot of hole 0 where its memo proves the snapshot current. A hit continues
    /// at TAKEN, past the helper step laid out after the row, which also takes the exhausted step's jump; a
    /// miss leaves through EXIT0 into it.
    FOR_IN_ROW,
    /// `SetFunctionName`'s fast arm (`staged::naming::site_name`): the function in the frame slot whose offset
    /// hole 2 carries (above `NAME_FUNCTION_AT`) takes the name word of hole 1 in the slot hole 2 carries (low
    /// `ADD_SLOT_BITS`), moving from the before-layout to the after-layout hole 0 carries as an add's does, where it
    /// held the empty name. One row per storage form the slot admits. A hit continues at TAKEN, past the helper
    /// step laid out after the row; a miss leaves through EXIT0 into it.
    NAME_EITHER_ROW,
    NAME_ARENA_ROW,
    /// The inline-cache rows (`ic_rows.rs`): a read of the receiver's own flat slot, a read of the slot its
    /// prototype holds, and a store of a Number into the receiver's own flat slot, whose slot type is `ANY` or
    /// `NUMBER` (`IC_SET_ROW`) or `INT32` (`IC_SET_INT_ROW`).
    IC_GET_ROW,
    IC_GET_HOLDER_ROW,
    IC_SET_ROW,
    IC_SET_INT_ROW,
    /// A read (`ENV_ROW`) and a store (`ENV_SET_ROW`) of a local slot the running activation's environment window holds
    /// (`staged::environment::environment_slot`), through the storage word the payload keeps
    /// (`JS_STEP_ENV_STORAGE`; one that holds no window reads as sparse storage, which every such row misses at): the slot
    /// at hole 1 and its byte offset at hole 2 as a capture row's, the destination (read) or the value operand (store) at hole 0.
    /// A hit continues at TAKEN, past the helper step laid out after the row; a slot past the run or holding an internal
    /// marker (a dead zone, a slot the environment took) leaves through EXIT0 into it.
    ENV_ROW,
    ENV_SET_ROW,
    /// The definition of a slot of that storage (`staged::environment::environment_slot_define`): a binding's first value
    /// (hole 0 the value operand, holes 1 and 2 the slot as `ENV_SET_ROW`'s), over its dead zone or an earlier value, which
    /// `InitializeTdz` (the hole as the operand) and a declaration's store make. A hit continues at TAKEN; a slot past the run
    /// or another environment's shared slot leaves through EXIT0 into the helper step.
    ENV_DEFINE_ROW,
    /// The destination (hole 0) takes the word of the global lexical binding at the address in hole 1 (`staged::lexical_read`), or the
    /// first operand (hole 1) is stored to it (address in hole 2) where it is initialized: a binding in its temporal dead zone (the deleted
    /// marker) leaves through EXIT0, so the interpreter's step raises. The address is held under the global layout, which a new declaration
    /// changes.
    LEXICAL_READ_ROW,
    LEXICAL_WRITE_ROW,
    /// A typed array's element of a planned kind (`TypedArrayKind` in discriminant order, `TYPED_KINDS` of them), read into
    /// the destination (`TYPED_INDEX_ROW` + kind) or replaced by the Number operand (`TYPED_STORE_ROW` + kind), through the
    /// view the loop's entry recorded: the entry words' record offset is hole 1 (a read) or hole 0 (a store), the index
    /// operand hole 2 and the store's value hole 1. The entry proved the kind once, so no row tests it; an index past the
    /// count or a value no element converts leaves through EXIT0 with nothing written. `TYPED_LENGTH_ROW` reads the count
    /// the entry words hold (hole 1) as the destination's int32.
    TYPED_INDEX_ROW,
    reserve TYPED_KINDS - 1,
    TYPED_STORE_ROW,
    reserve TYPED_KINDS - 1,
    TYPED_LENGTH_ROW,
    /// The receiver-proof rows (`ic_rows.rs`, `base_region::proofs`): the proof that the object register is of the
    /// entry's exact shape with flat values, and the guardless read and stores of a receiver every path proved so.
    IC_PROOF_ROW,
    IC_KNOWN_GET_ROW,
    IC_KNOWN_SET_ROW,
    IC_KNOWN_SET_INT_ROW,
    /// A declared parameter's entry test (`base_region::link::entry_rows`): the frame slot at hole 0 holds a word
    /// whose language its tag alone decides, in the mask of hole 1 (`shared_words::language`), where the mask
    /// is the declared set. A hit continues at NEXT; a miss leaves through EXIT0 with nothing written.
    DECLARED_ROW,
    /// `String.prototype.charCodeAt` of a flat string (hole 1: the receiver's frame slot) at an int32 index in range (hole 2):
    /// the code unit, an int32 into the destination (hole 0). Any other receiver, index or form leaves through EXIT0 with
    /// nothing written, to the call's own step.
    CHAR_CODE_AT_ROW,
    /// `parseInt` of a flat string (hole 1: the frame slot) in radix 10 (`staged::parse_int::decimal_int32`): the int32
    /// into the destination (hole 0). Any other text leaves through EXIT0 with nothing written.
    PARSE_INT_ROW,
}

/// The planned typed array kinds: each typed row family (`TYPED_INDEX_ROW`, `TYPED_STORE_ROW`) has one row per kind.
pub const TYPED_KINDS: u8 = 12;
/// The last row of each typed family.
pub const TYPED_INDEX_LAST: u8 = TYPED_INDEX_ROW + TYPED_KINDS - 1;
pub const TYPED_STORE_LAST: u8 = TYPED_STORE_ROW + TYPED_KINDS - 1;
/// The discriminants of the kinds with no native element (BigInt64, BigUint64, Float16), which have no row.
pub const NON_NATIVE_KINDS: [u8; 3] = [7, 8, 9];
/// Every naming row's code.
pub const NAME_ROWS: [u8; 2] = [NAME_EITHER_ROW, NAME_ARENA_ROW];
/// Bits below the function register's frame offset in a naming row's hole 2, which carries the slot beneath it.
pub const NAME_FUNCTION_AT: u32 = ADD_SLOT_BITS;
/// Every add row's code.
pub const ADD_ROWS: [u8; 6] = [ADD_PACKED_ROW, ADD_ARENA_ROW, ADD_EITHER_ROW, THIS_ADD_PACKED_ROW, THIS_ADD_ARENA_ROW, THIS_ADD_EITHER_ROW];
/// Bits below the after-shape in an add row's hole 0, which carries the before-shape beneath it.
pub const ADD_AFTER_AT: u32 = 32;
/// An add row's hole 2: the slot in its low `ADD_SLOT_BITS`, the prototype link a chained add proves
/// (a 28-bit header field, `OBJECT.proto_mask`) from `ADD_LINK_AT`, and the object register's frame
/// offset from `ADD_OBJECT_AT` (`ADD_OBJECT_BITS` of it: a register file's byte span fits).
pub const ADD_SLOT_BITS: u32 = u16::BITS;
pub const ADD_LINK_AT: u32 = ADD_SLOT_BITS;
pub const ADD_LINK_BITS: u32 = 28;
pub const ADD_OBJECT_AT: u32 = ADD_LINK_AT + ADD_LINK_BITS;
pub const ADD_OBJECT_BITS: u32 = u64::BITS - ADD_OBJECT_AT;
/// Bits below the prototype link in a folded read's hole 1, which carries the receiver's exact shape beneath it.
pub const HELD_LINK_AT: u32 = 32;
/// Bits below the object register's frame offset in a site field read's hole 0, which carries
/// the destination's frame offset beneath it.
pub const SITE_OBJECT_AT: u32 = 32;
/// Bits below the second cache entry's byte offset in a chained site read's hole 2, which carries
/// the first entry's offset beneath it.
pub const CHAIN_SECOND_AT: u32 = 32;

/// The kind families the library is built from.
pub const fn is_copy(kind: u8) -> bool {
    kind <= COPY
}
pub const fn is_arith(kind: u8) -> bool {
    ADD <= kind && kind <= MUL
}
pub const fn is_bitwise(kind: u8) -> bool {
    AND <= kind && kind <= USHR
}
pub const fn is_compare(kind: u8) -> bool {
    LT <= kind && kind <= NE
}
/// Whether a destination holds an int32 only: a pinned register or an int slot.
pub const fn holds_int(place: u8) -> bool {
    matches!(place, PIN..=LAST_INT_PIN | MEMINT)
}

/// What a supernode row runs first: a Number row or a heap row (`supernodes.rs`).
pub const FIRST_NUMBER: u8 = 0;
pub const FIRST_HEAP: u8 = 1;
/// A tested field store's hole 3 (`site_rows::site_store_row`): the tag fields past the first tag
/// a non-int32 word must fall outside. A slot whose observed type holds Number admits a double,
/// which lies outside the six tags; an int32-only slot admits none, against every tag field.
pub const STORE_ADMITS_DOUBLES: u64 = 6;
pub const STORE_ADMITS_INT32: u64 = 1 << 16;
