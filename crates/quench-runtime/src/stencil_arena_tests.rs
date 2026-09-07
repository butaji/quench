use super::*;
use crate::ir::Opcode;
use crate::quickening::QuickeningSite;
use crate::stencil_fact::{Hole, HoleKind, PatchValues, Stencil};

#[path = "stencil_arena_tests/cache.rs"]
mod cache;
#[path = "stencil_arena_tests/composition.rs"]
mod composition;
#[path = "stencil_arena_tests/generation.rs"]
mod generation;
#[path = "stencil_arena_tests/lease_lifetime.rs"]
mod lease_lifetime;
#[path = "stencil_arena_tests/native_bodies.rs"]
mod native_bodies;
#[path = "stencil_arena_tests/ownership.rs"]
mod ownership;
