//! Protocol misses of keyed reads and writes, deletes and named reads past their rows (`protocol_family::ELEMENT`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// `GetIndex`'s miss (`get_index_miss`).
pub(crate) const GET_INDEX: u8 = 0;
/// `SetIndex`'s miss (`set_index_miss`).
pub(crate) const SET_INDEX: u8 = 1;
/// `Delete`'s miss (`delete_miss`).
pub(crate) const DELETE: u8 = 2;
/// `GetField` at a getter site's miss (`get_field_getter_miss`).
pub(crate) const GET_FIELD_GETTER: u8 = 5;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row!(OP, ACC, { protocol_family::ELEMENT | parameters })`).
pub(crate) const fn element_miss_of(parameters: u8) -> Handler {
    match parameters {
        GET_INDEX => super::super::get_index_miss as Handler,
        SET_INDEX => super::super::set_index_miss as Handler,
        DELETE => super::super::delete_miss as Handler,
        GET_FIELD_GETTER => super::super::get_field_getter_miss as Handler,
        _ => general_step_in_lane,
    }
}
