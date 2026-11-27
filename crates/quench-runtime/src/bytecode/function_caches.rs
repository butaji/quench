//! The caches a function record fills on demand, and the booleans it derives on first ask: one
//! box for the rare fills, one word of bits for the flags, so a function nothing has asked about
//! pays for neither.

use super::{DeclaredAtomSets, Function, InitializerStore};

/// The caches a function fills only when something asks for them: a call derives its declared
/// atoms and initializer, a collection its register masks. Most functions never fill one, so
/// they share a box that the first fill allocates (`Function::caches`).
#[derive(Clone, Debug, Default)]
pub struct FunctionCaches {
    /// Lazily derived `declared_atoms`; never serialized.
    pub(crate) declared_atom_sets: std::cell::OnceCell<Box<DeclaredAtomSets>>,
    /// Lazily derived `initializer_stores`; never serialized.
    pub(crate) initializer: std::cell::OnceCell<Option<crate::heap::ThinBoxSlice<InitializerStore>>>,
    /// The live register masks, derived when a collection first finds the function running.
    pub(crate) register_masks: std::cell::OnceCell<std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    /// The int32 `+`/`-` PCs the facts analysis proves exact, ascending (`base_region::exact`), `None` where the
    /// analysis declines the function.
    pub exact_sums: std::cell::OnceCell<Option<Box<[u32]>>>,
    /// The proven-argument masks of b-jit compiles by their seeds (declared and pinned Number registers, pinned int32
    /// registers), at most `PROVEN_ARGS_SEEDS` of them (`base_region::call_args::ProvenArgs`).
    pub proven_args: std::cell::RefCell<Vec<ProvenArgsEntry>>,
    /// The call PCs that may replace this activation (`Function::permits_js_tail_call_at`), one bit per PC of
    /// `code`: the decision is static per site, so a call reads its bit instead of scanning the handlers.
    pub(crate) tail_sites: std::cell::OnceCell<Box<[u64]>>,
}

/// One seeding of `FunctionCaches::proven_args`: its Number registers, its int32 registers, the masks per PC.
pub type ProvenArgsEntry = (Box<[super::Register]>, Box<[super::Register]>, std::rc::Rc<[u64]>);

/// The seedings `FunctionCaches::proven_args` keeps; the oldest goes first.
pub const PROVEN_ARGS_SEEDS: usize = 4;

/// A boolean a function derives on its first ask and keeps (`Function::lazy_flag`). Each owns a
/// bit pair of `Function::lazy_flags`: its known bit, and its value bit above that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LazyFlag {
    ActivationBindings,
    MakerBindings,
    ArrowReads,
    LiveParameters,
    StaticNameScope,
    DynamicScopeAccess,
    EvalsInScope,
    JsTierCandidate,
    RestView,
    LocalsUseEnvironment,
}

/// The bits a lazy flag's pair spans: its known bit, then its value bit.
const LAZY_FLAG_PAIR_BITS: u32 = 2;

impl LazyFlag {
    const fn known_bit(self) -> u32 {
        1 << (self as u32 * LAZY_FLAG_PAIR_BITS)
    }

    const fn value_bit(self) -> u32 {
        self.known_bit() << 1
    }
}

impl Function {
    /// The caches this function fills on demand, boxed by the first fill.
    #[inline(always)] pub fn caches(&self) -> &FunctionCaches {
        self.caches.get_or_init(|| Box::default())
    }

    /// The derived boolean `flag`, derived by `derive` on its first ask and kept.
    #[inline(always)] pub fn lazy_flag(&self, flag: LazyFlag, derive: impl FnOnce() -> bool) -> bool {
        if self.lazy_flags.get() & flag.known_bit() == 0 {
            let value = derive();
            self.seed_lazy_flag(flag, value);
        }
        self.lazy_flags.get() & flag.value_bit() != 0
    }

    /// Records `flag` as `value` unless it is known already (`OnceCell::set`'s rule).
    pub(crate) fn seed_lazy_flag(&self, flag: LazyFlag, value: bool) {
        let bits = self.lazy_flags.get();
        if bits & flag.known_bit() == 0 {
            let value_bit = if value { flag.value_bit() } else { 0 };
            self.lazy_flags.set(bits | flag.known_bit() | value_bit);
        }
    }

    /// Whether the live register masks are derived (`RegisterMasks::for_collection`).
    pub(crate) fn register_masks_derived(&self) -> bool {
        self.caches.get().is_some_and(|caches| caches.register_masks.get().is_some())
    }

    /// The bytes of the live register masks derived so far.
    pub(crate) fn register_masks_bytes(&self) -> usize {
        self.caches.get().and_then(|caches| caches.register_masks.get()).map_or(0, |masks| masks.bytes())
    }
}
