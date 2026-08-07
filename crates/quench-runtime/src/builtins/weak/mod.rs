//! WeakSet and WeakMap built-ins.

pub mod registration;
pub mod weakmap;
pub mod weakset;

// Re-export helpers used by registration.
pub use weakmap::weakmap_entries_key;
pub use weakset::{is_callable, weakset_entries_key};

// Re-export registration for builtins/mod.rs.
pub use registration::register_weak_collections;
