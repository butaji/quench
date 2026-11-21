//! Protocol misses of keyed reads and writes, deletes and named reads past their rows (`protocol_family::ELEMENT`).

use super::super::*;

/// `GetIndex`'s miss (`get_index_miss`).
pub(in crate::vm::dispatch_fast) const GET_INDEX: u8 = 0;
/// `SetIndex`'s miss (`set_index_miss`).
pub(in crate::vm::dispatch_fast) const SET_INDEX: u8 = 1;
/// `Delete`'s miss (`delete_miss`).
pub(in crate::vm::dispatch_fast) const DELETE: u8 = 2;
/// `GetField` of a string primitive's miss (`get_field_string_miss`).
pub(in crate::vm::dispatch_fast) const GET_FIELD_STRING: u8 = 3;
/// `GetField` of a nested base's miss (`get_field_nested_miss`).
pub(in crate::vm::dispatch_fast) const GET_FIELD_NESTED: u8 = 4;
/// `GetField` at a getter site's miss (`get_field_getter_miss`).
pub(in crate::vm::dispatch_fast) const GET_FIELD_GETTER: u8 = 5;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<OP, ACC, { protocol_family::ELEMENT | parameters }>`).
pub(in crate::vm::dispatch_fast) const fn element_miss_of(parameters: u8) -> Handler {
    match parameters {
        GET_INDEX => super::super::get_index_miss as Handler,
        SET_INDEX => super::super::set_index_miss as Handler,
        DELETE => super::super::delete_miss as Handler,
        GET_FIELD_STRING => super::super::get_field_string_miss as Handler,
        GET_FIELD_NESTED => super::super::get_field_nested_miss as Handler,
        GET_FIELD_GETTER => super::super::get_field_getter_miss as Handler,
        _ => general_step_in_lane,
    }
}
