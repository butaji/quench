//! Handler rows of a lane: the handler selected for an opcode and the flattening of rows.
use super::*;

/// The handler serving opcode `OP` whose accumulator field is `ACC`, derived
/// from the opcode's semantic family; `None` when the general path executes
/// the opcode.
pub(in super::super) const fn handler_for<const OP: u16, const BACKWARD: bool, const ACC: u8>()
-> Option<LaneHandler> {
    let Some(op) = Op::from_index(OP as usize) else {
        return None;
    };
    // Past the first 256 opcodes only generated rows run in the lane; the rest leave it, as
    // before the table covered them, and instantiate nothing.
    if OP >= 256 && !admits_row(op) && !jumps_by_row(op) {
        return None;
    }
    if jumps_by_row(op) {
        return Some(LaneHandler { handler: row_jump!(OP, BACKWARD, FLAG_UNREAD, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }), produces: false });
    }
    if admits_row(op) {
        return Some(LaneHandler { handler: generated_row!(OP, ACC, FLAG_UNREAD), produces: row_produces(op) });
    }
    let handler: Handler = match other_handler(op) {
        Some(handler) => handler,
        None => return None,
    };
    Some(LaneHandler {
        handler,
        produces: false,
    })
}

/// The handlers of a class's private elements, none of which runs guest code: a field's
/// definition, a method's brand, and the brand checks of a read and of `#x in`.
pub(in super::super) const fn private_element_handler(opcode: Op) -> Option<Handler> {
    match opcode {
        _ if quench_runtime::staged::private_field_definition_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_FIELD_DEFINE as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::DEFINE_PRIVATE_FIELD as u16 }) as Handler),
        _ if quench_runtime::staged::private_name_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_NAME_MARK as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::MARK_PRIVATE_NAME as u16 }) as Handler),
        _ if quench_runtime::staged::private_check_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_CHECK as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::CHECK_PRIVATE as u16 }) as Handler),
        _ if quench_runtime::staged::private_in_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_IN as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::PRIVATE_IN as u16 }) as Handler),
        _ => None,
    }
}

/// Handlers that neither produce nor read the accumulator, apart from the
/// branch tests, which read it.
pub(in super::super) const fn other_handler(opcode: Op) -> Option<Handler> {
    match opcode {
        // The lane's synthetic prologue records: their staged rows, which the lane's activations start at.
        quench_runtime::bytecode::TIER_PROLOGUE => Some(tier_prologue_row()),
        quench_runtime::bytecode::BODY_ENTRY => Some(body_entry_row()),
        // A raising record unwinds in the lane where a handler takes it (`handler_edges::throw_record`).
        _ if opcode.raises() => Some(handler_edges::throw_record),
        // A derived constructor's `this` binding in its own frame (`derived_frames`).
        _ if opcode.binds_this() => Some(derived_frames::bind_this_in_frame),
        _ if quench_runtime::staged::field_read_family(opcode) => Some(row_get_field_first),
        // merge: theirs' `lane_set_object_field_own` / `lane_set_this_field_own` are undefined; HEAD's `field_store_row` serves both receivers.
        _ if opcode.field_store().is_some() => match opcode.field_store() { Some(store) => Some(field_store_row(store)), None => None },
        _ if quench_runtime::staged::property_record_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PROPERTY_RECORD_DEFINE as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::DEFINE_PROPERTY_RECORD as u16 }) as Handler),
        _ if quench_runtime::staged::field_definition_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::FIELD_DEFINE as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::DEFINE_FIELD as u16 }) as Handler),
        _ if private_element_handler(opcode).is_some() => private_element_handler(opcode),
        _ if quench_runtime::staged::copy_properties_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::COPY_DATA_PROPERTIES as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::COPY_DATA_PROPERTIES as u16 }) as Handler),
        _ if opcode.steps_in_lane() => Some(general_step_in_lane),
        _ if quench_runtime::staged::element_read_family(opcode) => Some(protocol_row!(ELEMENT_READ, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_INDEX as u16 }) as Handler),
        _ if quench_runtime::staged::element_write_family(opcode) => Some(protocol_row!(ELEMENT_WRITE, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::SET_INDEX as u16 }) as Handler),
        _ if quench_runtime::staged::known_call_family(opcode) => Some(known_call_handler()),
        _ if quench_runtime::staged::call_family(opcode) => Some(script_call_row(ANY_ARGC, false, false)),
        _ if quench_runtime::staged::call_array_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::CALL_ARRAY as u16 }, ACC_NONE, { protocol_family::NATIVE_CALL | protocol_misses::native_call::CALL_ARRAY as u16 }) as Handler),
        _ if quench_runtime::staged::call_spread_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::CALL_SPREAD as u16 }, ACC_NONE, { protocol_family::NATIVE_CALL | protocol_misses::native_call::CALL_SPREAD as u16 }) as Handler),
        _ if quench_runtime::staged::delete_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::DELETE_ROW as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::DELETE as u16 }) as Handler),
        _ if quench_runtime::staged::construction_family(opcode) => Some(protocol_handler::<{ quench_runtime::bytecode::CONSTRUCT_ROW as u16 }, ACC_NONE, { protocol_family::CONSTRUCT | protocol_misses::construct::SCRIPT_CONSTRUCT as u16 }>()),
        _ if quench_runtime::staged::forwarded_call_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::FORWARDED_CALL as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::FORWARD_CALL as u16 })),
        _ if quench_runtime::staged::return_family(opcode) => Some(return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(RETURN_KIND, NOT_LEAF))),
        _ if generator_yields::suspend_handler(opcode).is_some() => generator_yields::suspend_handler(opcode),
        _ => None,
    }
}

pub(in super::super) const fn flatten_rows(
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
