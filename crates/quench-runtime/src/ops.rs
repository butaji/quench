include!("ops/prelude.rs");
include!("ops/op.rs");
include!("ops/bodies.rs");
include!("ops/builtin.rs");
include!(concat!(env!("OUT_DIR"), "/op_variant_name.rs"));
pub(crate) mod meta;
