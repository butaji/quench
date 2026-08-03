//! Array method implementations

pub mod accessors;
pub mod grouping;
pub mod mutation;
pub mod rearrange;
pub mod search;
pub mod transformation;

pub use transformation::{call_callback, flatten_array, get_this_array, make_array};
pub use mutation::{get_this_array_obj, set_elements};
pub use accessors::{
    proto_at, proto_concat, proto_entries, proto_join, proto_keys, proto_slice, proto_to_string,
    proto_values,
};
pub use grouping::{proto_group_by, proto_group_by_to_map};
pub use mutation::{
    proto_pop, proto_push, proto_shift, proto_splice, proto_to_spliced, proto_unshift,
};
pub use rearrange::{
    proto_copy_within, proto_fill, proto_reverse, proto_sort, proto_to_reversed, proto_to_sorted,
};
pub use search::{
    proto_find, proto_find_index, proto_find_last, proto_find_last_index, proto_includes,
    proto_index_of, proto_last_index_of,
};
pub use transformation::{
    proto_every, proto_filter, proto_flat, proto_flat_map, proto_for_each, proto_map, proto_reduce,
    proto_reduce_right, proto_some,
};
