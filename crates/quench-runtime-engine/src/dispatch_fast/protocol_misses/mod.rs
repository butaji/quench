//! The protocol miss tables (`lane_frame::protocol_miss`): per family, the body that completes a
//! protocol record its row leaves unresolved, by the family's static parameters.

pub(in crate::dispatch_fast) mod element; pub(in crate::dispatch_fast) use element::element_miss_of;
pub(in crate::dispatch_fast) mod private; pub(in crate::dispatch_fast) use private::private_miss_of;
pub(in crate::dispatch_fast) mod script_call; pub(in crate::dispatch_fast) use script_call::script_call_miss_of;
pub(in crate::dispatch_fast) mod iteration; pub(in crate::dispatch_fast) use iteration::iteration_miss_of;
pub(in crate::dispatch_fast) mod native_call; pub(in crate::dispatch_fast) use native_call::native_call_miss_of;
pub(in crate::dispatch_fast) mod loops; pub(in crate::dispatch_fast) use loops::loop_miss_of;
pub(in crate::dispatch_fast) mod construct; pub(in crate::dispatch_fast) use construct::construct_miss_of;
pub(in crate::dispatch_fast) mod entry; pub(in crate::dispatch_fast) use entry::entry_miss_of;
mod returns; pub(in crate::dispatch_fast) use returns::{return_miss_of, returning_code, operator_return_code, RETURN_KIND, CONSTANT_KIND};
mod frame_call; pub(in crate::dispatch_fast) use frame_call::frame_call_miss_of;
pub(in crate::dispatch_fast) mod leaf_call; pub(in crate::dispatch_fast) use leaf_call::{leaf_call_miss_of, BUILTIN_KIND, MATH_KIND, REFLECT_GET_KIND};
mod vector; pub(in crate::dispatch_fast) use vector::vector_miss_of;
mod generator; pub(in crate::dispatch_fast) use generator::generator_miss_of;
