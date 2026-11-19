//! Capacity growth policies for the runtime's growable storage. Every growing container asks
//! `grown_capacity` for its next capacity, with the policy it uses, so each factor and floor
//! is named once here.
//!
//! The memory target is the reason growth factors matter: the heap is held to half of Node's
//! resident memory on the same workloads. A container keeps its growth slack resident after
//! it stops growing, so a factor that is too large leaves whole containers idle, while one that
//! is too small copies the container again at each step. Changing a policy changes the capacity
//! sequence, and so the RSS, of every container that uses it.
//!
//! The policies, and the containers that use each:
//!
//! - `GrowthPolicy::Half { minimum }`: array element backings (`heap::array_elements`) grow by
//!   half their capacity (`capacity + capacity / 2`), to at least `minimum` (4), as V8 grows fast
//!   elements. Appends stay amortized constant, and a grown backing holds slack of up to half its
//!   length rather than a full length as a doubling would leave.
//! - `GrowthPolicy::Double { minimum }`: collection entry tables (`heap::collection_entries`, the
//!   entries of Map, Set, WeakMap and WeakSet) double their slot count, to at least `minimum`
//!   (2). The table rounds the result up to the allocator's granule.
//! - `GrowthPolicy::SizeClass`: value-arena ranges (`value_vec`, the property vectors of objects)
//!   take the size class of their needed length (`capacity_class`: quarter steps per doubling),
//!   not a factor, so a range holds at most a fifth unused. A range that outgrows its class moves
//!   to the next one.
//! - `GrowthPolicy::PropertyBuffer { threshold, divisor, minimum }`: the property buffer
//!   (`value_vec::buffer`) doubles while it holds fewer than `threshold` values, and past that
//!   grows by `1 / divisor` of its capacity, to at least `minimum`. This is the arena's dense
//!   growth policy, which keeps large buffers from taking a full doubling in resident memory.

use crate::value_vec::capacity_class;

/// The growth factor and floor of a container, as its policy names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrowthPolicy {
    /// Grows by half the current capacity, to at least `minimum` (array element backings).
    Half { minimum: usize },
    /// Doubles the current capacity, to at least `minimum` (collection entry tables).
    Double { minimum: usize },
    /// The size class of the needed length; the current capacity is not a factor (value arena
    /// ranges, where the caller passes the length one past its capacity).
    SizeClass,
    /// Doubles below `threshold` values, then grows by `current / divisor`, to at least
    /// `minimum` (the property buffer).
    PropertyBuffer { threshold: usize, divisor: usize, minimum: usize },
}

/// The capacity a container of `current` capacity grows to so that it holds `needed` values,
/// under `policy`: the larger of `needed`, the policy's growth, and its floor.
#[inline]
pub(crate) fn grown_capacity(current: usize, needed: usize, policy: GrowthPolicy) -> usize {
    match policy {
        GrowthPolicy::Half { minimum } => needed.max(current + (current >> 1)).max(minimum),
        GrowthPolicy::Double { minimum } => needed.max(current * 2).max(minimum),
        GrowthPolicy::SizeClass => capacity_class(needed),
        GrowthPolicy::PropertyBuffer { threshold, divisor, minimum } => {
            let extra = if current < threshold { current } else { current / divisor };
            needed.max(current.saturating_add(extra)).max(minimum)
        }
    }
}
