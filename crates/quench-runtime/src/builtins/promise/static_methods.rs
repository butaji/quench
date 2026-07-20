//! Promise static methods — re-exports from submodules.

#[path = "capability.rs"]
pub mod capability;
#[path = "promise_all.rs"]
pub mod promise_all;
#[path = "promise_race.rs"]
pub mod promise_race;

pub use promise_all::{promise_all_impl, promise_reject_impl_static, promise_resolve_impl_static};
pub use promise_race::promise_race_impl;
