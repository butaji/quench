//! The residual judge of execution law 4: hot kernels with a known C equivalent, the counts that
//! C version needs per tier (`spec`), and the ratchet of what the tiers currently emit.
//!
//! A kernel's counts are taken over the innermost hot loop of the compiled artifact:
//! `boxes` (tag stores of numbers), `guards` (checks that leave the loop, the loop's own test
//! excluded), `spills` (loop-variable traffic through the frame or the stack), `calls`, and
//! `instructions` (CLIF instructions for the Cranelift tier, which AOT runs everywhere, and stencil
//! rows for the b-jit tier, which the default mode may pick for a function).

mod ratchet;
mod spec;

pub use ratchet::Ratchet;
pub use spec::KERNELS;

/// The categories of a count, in the order `Counts` stores them.
pub const CATEGORIES: [&str; CATEGORY_COUNT] =
    ["boxes", "guards", "spills", "calls", "instructions"];
pub const CATEGORY_COUNT: usize = 5;

/// The modes a kernel is judged in, in the order `Kernel::expect` stores them: the interpreter
/// (which compiles no loop), the default mode, and AOT.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    NoJit,
    Default,
    Aot,
}

impl Mode {
    pub const ALL: [Self; 3] = [Self::NoJit, Self::Default, Self::Aot];

    /// The name in reports and the ratchet (`no-jit` is the interpreter's flag without `--`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::NoJit => "no-jit",
            Self::Default => "default",
            Self::Aot => "aot",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }
}

/// One count per category.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts(pub [u32; CATEGORY_COUNT]);

impl Counts {
    /// The categories in which `self` is above `ceiling`.
    pub fn exceeding(self, ceiling: Self) -> impl Iterator<Item = &'static str> {
        CATEGORIES
            .into_iter()
            .zip(self.0.into_iter().zip(ceiling.0))
            .filter_map(|(name, (have, cap))| (have > cap).then_some(name))
    }

    /// The lower of each category.
    pub fn min(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| {
            self.0[index].min(other.0[index])
        }))
    }

    /// The count of the category `name`, if it is one.
    pub fn get(self, name: &str) -> Option<u32> {
        CATEGORIES
            .iter()
            .position(|category| *category == name)
            .map(|index| self.0[index])
    }
}

/// A kernel: JavaScript source defining `function kernel` and running it hot, with the counts
/// its C equivalent needs per mode.
pub struct Kernel {
    pub name: &'static str,
    pub source: &'static str,
    pub expect: [Counts; Mode::ALL.len()],
}

impl Kernel {
    pub fn expected(&self, mode: Mode) -> Counts {
        self.expect[mode as usize]
    }
}
