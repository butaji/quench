//! Protocol misses of private names and defined fields (`protocol_family::PRIVATE`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// `DefineField`'s miss (`define_field_miss`).
pub(crate) const DEFINE_FIELD: u8 = 0;
/// `DefinePrivateField`'s miss (`define_private_field_miss`).
pub(crate) const DEFINE_PRIVATE_FIELD: u8 = 1;
/// `MarkPrivateName`'s miss (`mark_private_name_miss`).
pub(crate) const MARK_PRIVATE_NAME: u8 = 2;
/// `DefinePropertyRecord`'s miss (`define_property_record_miss`).
pub(crate) const DEFINE_PROPERTY_RECORD: u8 = 3;
/// `CheckPrivate`'s miss (`check_private_miss`).
pub(crate) const CHECK_PRIVATE: u8 = 4;
/// `CopyDataProperties`'s miss (`copy_data_properties_miss`).
pub(crate) const COPY_DATA_PROPERTIES: u8 = 5;
/// `PrivateIn`'s miss (`private_in_miss`).
pub(crate) const PRIVATE_IN: u8 = 6;
/// `GetField` of a private name's miss (`get_private_field_miss`).
pub(crate) const GET_PRIVATE_FIELD: u8 = 7;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row!(OP, ACC, { protocol_family::PRIVATE | parameters })`).
pub(crate) const fn private_miss_of(parameters: u8) -> Handler {
    match parameters {
        DEFINE_FIELD => super::super::define_field_miss as Handler,
        DEFINE_PRIVATE_FIELD => super::super::define_private_field_miss as Handler,
        MARK_PRIVATE_NAME => super::super::mark_private_name_miss as Handler,
        DEFINE_PROPERTY_RECORD => super::super::define_property_record_miss as Handler,
        CHECK_PRIVATE => super::super::check_private_miss as Handler,
        COPY_DATA_PROPERTIES => super::super::copy_data_properties_miss as Handler,
        PRIVATE_IN => super::super::private_in_miss as Handler,
        GET_PRIVATE_FIELD => super::super::get_private_field_miss as Handler,
        _ => general_step_in_lane,
    }
}
