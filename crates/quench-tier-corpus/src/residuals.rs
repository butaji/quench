//! The residual judge of execution law 4: hot kernels with a known C equivalent, the counts that
//! C version needs per tier (`spec`), and the ratchet of what the tiers currently emit.
//!
//! A kernel's counts are taken over the innermost hot loop of the compiled artifact:
//! `boxes` (tag stores of numbers), `guards` (checks that leave the loop, the loop's own test
//! excluded), `spills` (loop-variable traffic through the frame or the stack), `calls`, and
//! `instructions` (CLIF instructions for u-jit, stencil rows for b-jit).

mod ratchet;
mod spec;

pub use ratchet::Ratchet;
pub use spec::KERNELS;

/// The categories of a count, in the order `Counts` stores them.
pub const CATEGORIES: [&str; CATEGORY_COUNT] =
    ["boxes", "guards", "spills", "calls", "instructions"];
pub const CATEGORY_COUNT: usize = 5;

/// The tiers a kernel is judged on, in the order `Kernel::expect` stores them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    UJit,
    BJit,
}

impl Tier {
    pub const ALL: [Self; 2] = [Self::UJit, Self::BJit];

    /// The name in reports and the ratchet, which is also the tier's command-line flag without `--`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::UJit => "u-jit",
            Self::BJit => "b-jit",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.name() == name)
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
/// its C equivalent needs per tier.
pub struct Kernel {
    pub name: &'static str,
    pub source: &'static str,
    pub expect: [Counts; Tier::ALL.len()],
}

impl Kernel {
    pub fn expected(&self, tier: Tier) -> Counts {
        self.expect[tier as usize]
    }
}
