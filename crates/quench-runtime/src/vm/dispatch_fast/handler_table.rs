//! The lane handler table: the handler each opcode and accumulator field selects.

use super::*;

/// The handler serving opcode `OP` whose accumulator field is `ACC`, derived
/// from the opcode's semantic family; `None` when the general path executes
/// the opcode.
pub(super) const fn handler_for<H: Host, const OP: u16, const BACKWARD: bool, const ACC: u8>()
-> Option<LaneHandler> {
    let Some(op) = Op::from_index(OP as usize) else {
        return None;
    };
    if I32BinaryOperator::from_register_op(op).is_some()
        || I32BinaryOperator::from_immediate_op(op).is_some()
    {
        return LaneHandler::producer(lane_i32_binary::<H, OP, ACC>);
    }
    let handler: Handler = if I32BinaryOperator::from_jump_op(op).is_some()
        || I32BinaryOperator::from_immediate_jump_op(op).is_some()
    {
        lane_compare_jump::<H, OP, BACKWARD, ACC>
    } else if MemoryLoad::from_direct_op(op).is_some() {
        return LaneHandler::producer(lane_load::<H, OP, ACC>);
    } else if MemoryStore::from_direct_op(op).is_some() {
        lane_store::<H, OP, ACC>
    } else if let Some(producer) = class_producer::<H, OP, BACKWARD, ACC>(op) {
        return producer;
    } else {
        match other_handler::<H, BACKWARD, ACC>(op) {
            Some(handler) => handler,
            None => return None,
        }
    };
    Some(LaneHandler {
        handler,
        produces: false,
    })
}

/// The producer of a class that writes a register (`LaneHandler::producer`), as `handler_for`
/// takes it: `Some` of the answer for each such class, `None` for every other class.
const fn class_producer<H: Host, const OP: u16, const BACKWARD: bool, const ACC: u8>(op: Op) -> Option<Option<LaneHandler>> {
    match op.class() {
        Class::Move => Some(LaneHandler::producer(lane_move::<H, ACC>)),
        Class::LoadConst => Some(LaneHandler::producer(lane_load_const::<H>)),
        Class::WasmI32ShiftRightUnsignedAnd => Some(LaneHandler::producer(lane_bit_field::<H, ACC>)),
        Class::WasmSelect => Some(LaneHandler::producer(lane_select::<H, ACC>)),
        Class::WasmI32AddJump => Some(LaneHandler::producer(lane_add_jump_nonzero::<H, BACKWARD, ACC>)),
        Class::WasmI32Unary => Some(LaneHandler::producer(lane_i32_unary::<H, ACC>)),
        Class::WasmFloatBinary32 | Class::WasmFloatBinary64 => Some(LaneHandler::producer(lane_float_binary::<H, OP, ACC>)),
        Class::WasmFloatUnary32 | Class::WasmFloatUnary64 => Some(LaneHandler::producer(lane_float_unary::<H, OP, ACC>)),
        Class::WasmI64Binary => Some(LaneHandler::producer(lane_i64_binary::<H, ACC>)),
        Class::WasmI64Unary => Some(LaneHandler::producer(lane_i64_unary::<H, ACC>)),
        Class::WasmScalarConvert => Some(LaneHandler::producer(lane_scalar_convert::<H, ACC>)),
        _ => None,
    }
}

/// The handlers of the jumps (`other_handler`), whose direction decides whether they run a
/// safepoint, then of the Wasm GC steps (`wasm_gc_handler`).
pub(super) const fn jump_handler<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::JumpBinaryFalse => Some(lane_jump_binary_false::<H, BACKWARD>),
        Class::Jump => Some(lane_jump_always::<H, BACKWARD>),
        Class::JumpFalse => Some(lane_jump_false::<H, BACKWARD>),
        Class::JumpTrue => Some(lane_jump_true::<H, BACKWARD>),
        Class::WasmBranchTable => Some(lane_branch_table::<H, BACKWARD>),
        _ if opcode.traits().has(crate::bytecode::Traits::LANE_LOOP_EXIT) => Some(for_in_steps::for_in_next_handler::<H, BACKWARD>()),
        Class::WasmJumpI32Zero => Some(lane_jump_i32_zero::<H, true, BACKWARD, ACC, false>),
        Class::WasmJumpI32NonZero => Some(lane_jump_i32_zero::<H, false, BACKWARD, ACC, false>),
        _ => wasm_gc_handler::<H>(opcode),
    }
}

/// The handlers of a class's private elements, none of which runs guest code: a field's
/// definition, a method's brand, and the brand checks of a read and of `#x in`.
pub(super) const fn private_element_handler<H: Host>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::DefinePrivateField => Some(lane_define_private_field::<H>),
        Class::MarkPrivateName => Some(lane_mark_private_name::<H>),
        Class::CheckPrivate => Some(lane_check_private::<H>),
        Class::PrivateIn => Some(lane_private_in::<H>),
        _ => None,
    }
}

/// The handlers of the iterator protocol's steps: an array's intrinsic iterator, a close
/// that ends without a call, and the bookkeeping of the iterators a frame must close.
pub(super) const fn iterator_handler<H: Host>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::GetIterator => Some(lane_get_iterator::<H>),
        Class::IteratorClose => Some(lane_iterator_close::<H>),
        Class::AppendArray => Some(lane_append_array::<H>),
        Class::IteratorCleanupPush => Some(lane_iterator_cleanup_push::<H>),
        Class::RequireIteratorResult => Some(lane_require_iterator_result::<H>),
        Class::IteratorCleanupPop => Some(lane_iterator_cleanup_pop::<H>),
        _ => None,
    }
}

/// Handlers that neither produce nor read the accumulator, apart from the
/// branch tests, which read it.
/// Handlers that neither produce nor read the accumulator, apart from the
/// branch tests, which read it. The classes are matched in groups, each falling through to the next.
pub(super) const fn other_handler<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::StoreLocalPlain => Some(lane_store_local_plain::<H>),
        Class::StoreName => Some(lane_store_name_first::<H>),
        Class::DefineArrayElement => Some(lane_define_array_element::<H>),
        Class::StoreLocal | Class::StoreEnvLocal => Some(lane_store_local::<H>),
        Class::LoadThis => Some(lane_load_this::<H>),
        Class::LoadCallee => Some(lane_load_callee::<H>),
        _ => other_handler_1::<H, BACKWARD, ACC>(opcode),
    }
}

const fn other_handler_1<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::InitializeTdz => Some(lane_initialize_tdz::<H>),
        Class::CloneEnv => Some(lane_clone_env::<H>),
        Class::GetField => Some(lane_get_field_first::<H>),
        _ if matches!(opcode.field_store(), Some(FieldStore { receiver: StoreReceiver::Register, .. })) => Some(lane_set_object_field_own::<H, false>),
        _ if matches!(opcode.field_store(), Some(FieldStore { receiver: StoreReceiver::This, .. })) => Some(lane_set_this_field_own::<H, false>),
        Class::ToNumeric => Some(lane_to_numeric::<H>),
        _ => other_handler_2::<H, BACKWARD, ACC>(opcode),
    }
}

const fn other_handler_2<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::ToPropertyKey => Some(lane_to_property_key::<H>),
        Class::RequireObjectCoercible => Some(lane_require_object_coercible::<H>),
        Class::DefinePropertyRecord => Some(lane_define_property_record::<H>),
        Class::DefineField => Some(lane_define_field::<H>),
        _ if private_element_handler::<H>(opcode).is_some() => private_element_handler::<H>(opcode),
        Class::CopyDataProperties => Some(lane_copy_data_properties::<H>),
        _ => other_handler_3::<H, BACKWARD, ACC>(opcode),
    }
}

const fn other_handler_3<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        _ if iterator_handler::<H>(opcode).is_some() => iterator_handler::<H>(opcode),
        Class::ForInStart => Some(for_in_steps::for_in_start_handler::<H>()),
        _ if opcode.steps_in_lane() => Some(lane_general_step::<H>),
        Class::IncDec => Some(lane_inc_dec::<H, false>),
        _ if denoted_handler::<H>(opcode).is_some() => denoted_handler::<H>(opcode),
        Class::GetIndex => Some(lane_get_index::<H>),
        _ => other_handler_4::<H, BACKWARD, ACC>(opcode),
    }
}

const fn other_handler_4<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::SetIndex => Some(lane_set_index::<H>),
        Class::LoadCapture => Some(lane_load_capture::<H>),
        Class::LoadLocalPlain => Some(lane_load_local_plain::<H>),
        Class::MakeConstArray => Some(lane_make_const_array::<H>),
        Class::StringCaseIndex => Some(lane_string_case_index::<H>),
        Class::LoadLocal | Class::LoadEnvLocal => Some(lane_load_local::<H>),
        _ => other_handler_5::<H, BACKWARD, ACC>(opcode),
    }
}

const fn other_handler_5<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::StoreCapture => Some(lane_store_capture::<H>),
        Class::WasmInstanceBinding => Some(lane_instance_binding::<H, false>),
        Class::WasmGlobalGet => Some(lane_global_get::<H>),
        Class::WasmSimd => Some(lane_wasm_simd::<H>),
        Class::WasmMemoryLoad => Some(lane_wasm_memory_load::<H>),
        Class::WasmMemoryAddress => Some(lane_wasm_memory_address::<H>),
        _ => other_handler_6::<H, BACKWARD, ACC>(opcode),
    }
}

const fn other_handler_6<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::WasmMemoryStore => Some(lane_wasm_memory_store::<H>),
        Class::WasmSimdShuffle => Some(lane_wasm_simd_shuffle::<H>),
        Class::WasmGlobalSet => Some(lane_global_set::<H>),
        Class::WasmFillRegisters => Some(lane_fill_registers::<H>),
        Class::CallKnown => Some(known_call_handler::<H>()),
        Class::Call => Some(lane_script_call::<H, ANY_ARGC, false>),
        _ => other_handler_7::<H, BACKWARD, ACC>(opcode),
    }
}

const fn other_handler_7<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::CallArray => Some(lane_call_array::<H>),
        Class::CallSpread => Some(lane_call_spread::<H>),
        Class::Delete => Some(lane_delete::<H>),
        Class::Construct => Some(lane_script_construct::<H>),
        Class::LoadName => Some(lane_load_name_resolve::<H, LOAD_NAME>),
        Class::LoadNameCall => Some(lane_load_name_resolve::<H, LOAD_NAME_CALL>),
        _ => other_handler_8::<H, BACKWARD, ACC>(opcode),
    }
}

const fn other_handler_8<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::LoadNameTypeof => Some(lane_load_name_resolve::<H, LOAD_NAME_TYPEOF>),
        Class::MakeObject2 => Some(lane_make_object2::<H>),
        Class::MakeClosure => Some(lane_make_closure::<H>),
        Class::MakeObjectLiteral => Some(lane_make_object_literal::<H>),
        Class::MakeArray => Some(lane_make_array::<H>),
        Class::MakeObject => Some(lane_make_object::<H>),
        _ => other_handler_9::<H, BACKWARD, ACC>(opcode),
    }
}

const fn other_handler_9<H: Host, const BACKWARD: bool, const ACC: u8>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::CreateRegExpLiteral => Some(lane_regexp_literal::<H>),
        Class::CallForwardingArguments => Some(lane_forward_call::<H>),
        Class::SuperConstArrayObject2 => Some(lane_const_array_object2::<H>),
        Class::Return => Some(lane_return::<H, NOT_LEAF>),
        _ if suspend_handler::<H>(opcode).is_some() => suspend_handler::<H>(opcode),
        _ => jump_handler::<H, BACKWARD, ACC>(opcode),
    }
}

/// Dense fast handlers cover the first opcode byte; later opcodes use general dispatch.
pub(super) const TABLE_SLOTS: usize = 1 << u8::BITS;

/// Opcodes per generated table row.
pub(super) const TABLE_ROW: u16 = 16;
pub(super) const TABLE_ROWS: usize = TABLE_SLOTS / TABLE_ROW as usize;

macro_rules! table_row {
    ($h:ty, $backward:literal, $acc:expr, $row:literal) => {
        [
            handler_for::<$h, { $row * TABLE_ROW }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 1 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 2 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 3 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 4 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 5 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 6 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 7 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 8 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 9 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 10 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 11 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 12 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 13 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 14 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 15 }, $backward, { $acc }>(),
        ]
    };
}

pub(super) const fn flatten_rows(
    rows: [[Option<LaneHandler>; TABLE_ROW as usize]; TABLE_ROWS],
) -> [Option<LaneHandler>; TABLE_SLOTS] {
    let mut table = [None; TABLE_SLOTS];
    let mut slot = 0;
    while slot < TABLE_SLOTS {
        table[slot] = rows[slot / TABLE_ROW as usize][slot % TABLE_ROW as usize];
        slot += 1;
    }
    table
}

/// One handler table: the opcode's handler for one jump direction and one
/// accumulator field.
pub(super) type Table = [Option<LaneHandler>; TABLE_SLOTS];

macro_rules! lane_table {
    ($h:ty, $backward:literal, $acc:expr) => {
        flatten_rows([
            table_row!($h, $backward, $acc, 0),
            table_row!($h, $backward, $acc, 1),
            table_row!($h, $backward, $acc, 2),
            table_row!($h, $backward, $acc, 3),
            table_row!($h, $backward, $acc, 4),
            table_row!($h, $backward, $acc, 5),
            table_row!($h, $backward, $acc, 6),
            table_row!($h, $backward, $acc, 7),
            table_row!($h, $backward, $acc, 8),
            table_row!($h, $backward, $acc, 9),
            table_row!($h, $backward, $acc, 10),
            table_row!($h, $backward, $acc, 11),
            table_row!($h, $backward, $acc, 12),
            table_row!($h, $backward, $acc, 13),
            table_row!($h, $backward, $acc, 14),
            table_row!($h, $backward, $acc, 15),
        ])
    };
}

pub(super) struct LaneTable<H>(std::marker::PhantomData<H>);

impl<H: Host> LaneTable<H> {
    /// Tables indexed by jump direction (forward, then backward: a
    /// safepoint) and by accumulator field.
    const TABLES: [[Table; ACC_POSITIONS]; 2] = [
        [
            lane_table!(H, false, ACC_NONE),
            lane_table!(H, false, ACC_A),
            lane_table!(H, false, ACC_B),
            lane_table!(H, false, ACC_C),
            lane_table!(H, false, ACC_PAIR),
        ],
        [
            lane_table!(H, true, ACC_NONE),
            lane_table!(H, true, ACC_A),
            lane_table!(H, true, ACC_B),
            lane_table!(H, true, ACC_C),
            lane_table!(H, true, ACC_PAIR),
        ],
    ];
}
