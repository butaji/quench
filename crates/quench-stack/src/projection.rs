//! The projections a staged row can run on, and the seal that makes them the only ones. Every
//! stage-like trait (`Stage`, `MachineStage`, `MatchStage`) requires `Sealed`, so a stage type
//! that names no projection does not compile. `projection!` is the only way to implement
//! `Sealed`: it accepts `Concrete` (the interpreter), `StencilStage` (the b-jit's rustc stencils)
//! and `Emit` (the u-jit and AOT's Cranelift IR); any other name is a compile error. The enum has
//! three variants, so a new language or sub-domain is a type parameter of one of them, not a fourth.
//!
//! Rust cannot seal a trait against other crates by visibility, so the rule is also a convention:
//! nothing writes `impl Sealed` by hand except the generic types `projection!` cannot name (see
//! `quench-regexp`'s `Concrete` and the regexp `Emit`). The b-jit stencil build includes this file
//! by path (`quench-bjit/stencils/rows.rs`), so it uses no crate but `core`.

/// The projections a staged row can run on.
#[allow(dead_code)] // each crate names only the variants it defines
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Projection {
    /// The interpreter: machine words of live memory, and the recording observer of it.
    Concrete,
    /// The b-jit: rustc/LLVM stencils that the stitcher patches.
    StencilStage,
    /// The u-jit and AOT: Cranelift IR.
    Emit,
}

/// Implemented only by `projection!`, which sets the projection the type is.
#[allow(dead_code)] // read by no code: the record of which projection a type is
pub trait Sealed {
    /// The projection this type is.
    const PROJECTION: Projection;
}

/// Declares a type to be one of the three projections, which is the only way to make it a stage
/// of any kind: `projection!(ConcreteWasm => Concrete)`. Any other projection name is a compile
/// error, and so is a type that is not a `Sealed` implementor.
#[macro_export]
macro_rules! projection {
    ($ty:ty => Concrete) => { $crate::projection!(@seal $ty, Concrete); };
    ($ty:ty => StencilStage) => { $crate::projection!(@seal $ty, StencilStage); };
    ($ty:ty => Emit) => { $crate::projection!(@seal $ty, Emit); };
    ($ty:ty => $other:ident) => {
        compile_error!(concat!("projection!: `", stringify!($other), "` is not one of the three projections (Concrete, StencilStage, Emit)"));
    };
    (@seal $ty:ty, $projection:ident) => {
        impl $crate::projection::Sealed for $ty {
            const PROJECTION: $crate::projection::Projection = $crate::projection::Projection::$projection;
        }
    };
}
