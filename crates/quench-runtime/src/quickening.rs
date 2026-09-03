//! Bounded, catalog-backed quickening state.
//!
//! A site owns only disposable physical state. It never owns JavaScript
//! semantics: an install is reported to the caller, while a miss remains an
//! ordinary fallback at the same operation site.

use crate::facts::{Certainty, OperationGuard, WordKind};
use crate::ir::Opcode;
use crate::shape_cache::{PropertyId, ShapeCache, ShapeId};

/// Shared bounded-degrade limit for interpreter and stencil installations.
/// Keeping one public constant makes the lifecycle bound auditable and avoids
/// a second, drifting tier policy.
pub const MAX_MISSES: u8 = 3;

/// Physical cache tier. The megamorphic tier is still bounded and may hit
/// any retained guard; it only describes admission diversity, not a second
/// semantic implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickeningTier {
    Cold,
    Polymorphic,
    Megamorphic,
}

/// Physical action selected by a quickening site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickeningDecision {
    /// Execute the guarded physical operation.
    GuardedHit { slot: u32 },
    /// Execute a cached direct call for the same callable identity.
    GuardedCallHit,
    /// Record a guard, then execute the complete fallback for this access.
    InstallGuard { slot: u32 },
    /// Record a callable identity, then execute the complete call gateway.
    InstallCallGuard,
    /// No physical specialization is admitted; use ordinary semantics.
    Fallback,
}

/// Result of the generic inline-cache key/state phase.  The cache never owns
/// the effectful operation: callers receive the state and must apply it using
/// the complete semantic path.  This mirrors the Deegen split
/// `lambda_i: key -> state` / `lambda_e: (input, state) -> output` without
/// introducing a second representation of JavaScript values.
#[derive(Debug, Clone, PartialEq)]
pub enum GenericIcDecision<S> {
    Hit(S),
    Install(S),
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GenericIcEntry<K, S> {
    key: K,
    state: S,
}

/// A bounded, reusable generic IC.  `derive_state` is called only on a miss;
/// its result is disposable physical state, while the caller remains the
/// semantic owner of the effect.  Once full, entries are replaced round-robin
/// rather than allowing unbounded cache growth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenericInlineCache<K, S, const N: usize = 4> {
    entries: [Option<GenericIcEntry<K, S>>; N],
    next_replacement: usize,
}

impl<K, S, const N: usize> GenericInlineCache<K, S, N>
where
    K: Eq + Clone,
    S: Clone,
{
    pub fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
            next_replacement: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    pub const fn capacity(&self) -> usize {
        N
    }

    pub fn lookup(&self, key: &K) -> Option<S> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.key == *key)
            .map(|entry| entry.state.clone())
    }

    /// Borrow the physical key/state pairs without exposing the backing
    /// array.  Specialized cache views use this to derive their own compact
    /// entry types while retaining one storage mechanism.
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&K, &S)> {
        self.entries
            .iter()
            .flatten()
            .map(|entry| (&entry.key, &entry.state))
    }

    /// Promote a hit without changing capacity or semantic state. Preserve
    /// the entry selected by the round-robin cursor: swapping entries must
    /// also swap the cursor's physical index, otherwise a hot entry could be
    /// evicted immediately after promotion.
    pub(crate) fn promote(&mut self, key: &K) {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|entry| entry.key == *key))
        else {
            return;
        };
        if index != 0 {
            self.entries.swap(0, index);
            if self.next_replacement == 0 {
                self.next_replacement = index;
            } else if self.next_replacement == index {
                self.next_replacement = 0;
            }
        }
    }

    /// Remove all physical entries matching a predicate.  Invalidation is
    /// deliberately a cache effect; the canonical operation remains intact.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &S) -> bool) {
        for entry in &mut self.entries {
            let remove = entry
                .as_ref()
                .is_some_and(|entry| !keep(&entry.key, &entry.state));
            if remove {
                *entry = None;
            }
        }
    }

    /// Run the idempotent key→state phase and return a state for the caller's
    /// effect phase. `None` means the operation is not cacheable and must use
    /// its ordinary fallback.
    pub fn observe(
        &mut self,
        key: K,
        derive_state: impl FnOnce(&K) -> Option<S>,
    ) -> GenericIcDecision<S> {
        if let Some(state) = self.lookup(&key) {
            // Keep the hottest physical case at the front of the bounded
            // chain. The cache preserves the replacement target while doing
            // so, so this changes only probe order.
            self.promote(&key);
            return GenericIcDecision::Hit(state);
        }
        if N == 0 {
            return GenericIcDecision::Fallback;
        }
        let Some(state) = derive_state(&key) else {
            return GenericIcDecision::Fallback;
        };
        self.insert_state(key, state.clone());
        GenericIcDecision::Install(state)
    }

    /// Execute the generic IC's complete probe/apply split without allocating
    /// a second semantic representation. `derive_state` is the idempotent
    /// key→state phase and runs only on a miss; `effect` is the cheap
    /// input/state phase used for both hits and newly installed entries.
    /// `fallback` owns the complete operation when the key is uncacheable or
    /// the bounded cache has no capacity.
    pub fn execute<I, T>(
        &mut self,
        input: I,
        key: K,
        derive_state: impl FnOnce(&K) -> Option<S>,
        effect: impl FnOnce(I, S) -> T,
        fallback: impl FnOnce(I) -> T,
    ) -> T {
        if let Some(state) = self.lookup(&key) {
            // Generic IC execution is the polymorphic-chain view. Promote a
            // hit without changing capacity or semantic state so the common
            // case is encountered first on subsequent probes.
            self.promote(&key);
            return effect(input, state);
        }
        if N == 0 {
            return fallback(input);
        }
        let Some(state) = derive_state(&key) else {
            return fallback(input);
        };
        self.insert_state(key, state.clone());
        effect(input, state)
    }

    /// Insert or update one physical key/state pair. Updating an existing
    /// pair does not consume a replacement turn, which keeps cache behavior
    /// stable for specialized views such as shape/property slots.
    pub(crate) fn insert_state(&mut self, key: K, state: S) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.key == key)
        {
            entry.state = state;
            return;
        }
        if N == 0 {
            return;
        }
        self.entries[self.next_replacement] = Some(GenericIcEntry { key, state });
        self.next_replacement = (self.next_replacement + 1) % N;
    }

    pub fn clear(&mut self) {
        self.entries.fill(None);
        self.next_replacement = 0;
    }
}

impl<K, S, const N: usize> Default for GenericInlineCache<K, S, N>
where
    K: Eq + Clone,
    S: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Bounded state attached to one operation site.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickeningSite<const N: usize = 4> {
    opcode: Opcode,
    certainty: Certainty,
    cache: ShapeCache<N>,
    callable_cache: CallableCache<N>,
    misses: u8,
    stable_hits: u8,
    tier: QuickeningTier,
}

impl<const N: usize> QuickeningSite<N> {
    pub fn new(opcode: Opcode) -> Self {
        Self {
            opcode,
            certainty: Certainty::Unknown,
            cache: ShapeCache::new(),
            callable_cache: CallableCache::new(),
            misses: 0,
            stable_hits: 0,
            tier: QuickeningTier::Cold,
        }
    }

    pub fn opcode(&self) -> Opcode {
        self.opcode
    }

    pub fn certainty(&self) -> Certainty {
        self.certainty
    }

    pub fn misses(&self) -> u8 {
        self.misses
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    pub fn callable_cache_len(&self) -> usize {
        self.callable_cache.len()
    }

    pub const fn tier(&self) -> QuickeningTier {
        self.tier
    }

    /// Stable identity for bytes that may embed the site's physical IC state.
    /// The digest includes the bounded entries themselves rather than only
    /// their count, so two different shape/callable chains cannot share a
    /// rendered region accidentally.
    pub(crate) fn patch_signature(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        hash = mix_signature(hash, self.opcode as u64);
        hash = mix_signature(hash, self.certainty as u64);
        hash = mix_signature(hash, u64::from(self.misses));
        for entry in self.cache.entries() {
            hash = mix_signature(hash, u64::from(entry.guard_shape.0));
            hash = mix_signature(hash, u64::from(entry.property.0));
            hash = mix_signature(hash, u64::from(entry.slot));
        }
        for entry in self.callable_cache.entries.iter() {
            let pointer = entry
                .as_ref()
                .map_or(0, |weak| weak.as_ptr() as usize as u64);
            hash = mix_signature(hash, pointer);
        }
        hash
    }

    /// Probe a shape/property guard without deriving its slot. This is the
    /// idempotent-cache half of generic IC lowering: callers can skip the
    /// potentially expensive semantic lookup on a hit and apply the cached
    /// state themselves.
    pub fn probe_shape(&mut self, shape: ShapeId, property: PropertyId) -> Option<u32> {
        if !self.opcode.is_quickenable()
            || self.opcode.guarded_word_kind(OperationGuard::Shape) != Some(WordKind::Object)
        {
            return None;
        }
        let cached_slot = self.cache.lookup(shape, property)?;
        crate::execution_trace::quickening_observation(self.opcode, true);
        if crate::execution_trace::quickening_prefers_hot(self.opcode) {
            self.cache.promote(shape, property);
        }
        self.certainty = Certainty::Guarded;
        self.stable_hit();
        Some(cached_slot)
    }

    /// Admit one shape/property observation without executing semantics.
    ///
    /// Shape ICs are admitted only when the operation row declares the
    /// `Shape` guard. Other guard kinds have their own future physical views.
    ///
    /// This is the pure key-check half of the inline-cache split: it only
    /// decides `GuardedHit`/`InstallGuard`/`Fallback` and updates cache
    /// bookkeeping. The effectful apply (the property slot read/write) stays
    /// in the caller, e.g. `quickened_own_get` in `vm/vm_runtime.rs`. Keep
    /// this split intact — do not fold the slot read/write into `observe`.
    pub fn observe(
        &mut self,
        shape: ShapeId,
        property: PropertyId,
        slot: u32,
    ) -> QuickeningDecision {
        if !self.opcode.is_quickenable()
            || self.opcode.guarded_word_kind(OperationGuard::Shape) != Some(WordKind::Object)
        {
            return QuickeningDecision::Fallback;
        }
        if let Some(cached_slot) = self.probe_shape(shape, property) {
            return QuickeningDecision::GuardedHit { slot: cached_slot };
        }
        if N == 0 {
            self.certainty = Certainty::Unknown;
            return QuickeningDecision::Fallback;
        }
        self.record_miss();
        crate::execution_trace::quickening_observation(self.opcode, false);
        self.cache.insert(shape, property, slot);
        self.certainty = Certainty::Guarded;
        QuickeningDecision::InstallGuard { slot }
    }

    /// Invalidate every guard that depends on a shape epoch.
    pub fn invalidate_shape(&mut self, shape: ShapeId) {
        self.cache.invalidate_shape(shape);
        if self.cache.is_empty() {
            self.certainty = Certainty::Unknown;
        }
    }

    /// Admit one callable identity for a direct-call fast path.
    ///
    /// The cache stores a weak identity, never a semantic implementation or
    /// an owning reference. A weak edge also prevents allocator address reuse
    /// from turning a stale entry into an unsafe call target.
    pub fn observe_callable(
        &mut self,
        function: &std::rc::Rc<crate::value::FunctionValue>,
    ) -> QuickeningDecision {
        if !self.opcode.is_quickenable()
            || self.opcode.guarded_word_kind(OperationGuard::Callable) != Some(WordKind::Callable)
        {
            return QuickeningDecision::Fallback;
        }
        if self.callable_cache.lookup(function) {
            crate::execution_trace::quickening_observation(self.opcode, true);
            if crate::execution_trace::quickening_prefers_hot(self.opcode) {
                self.callable_cache.promote(function);
            }
            self.certainty = Certainty::Guarded;
            self.stable_hit();
            return QuickeningDecision::GuardedCallHit;
        }
        if N == 0 {
            self.certainty = Certainty::Unknown;
            return QuickeningDecision::Fallback;
        }
        self.record_miss();
        crate::execution_trace::quickening_observation(self.opcode, false);
        self.callable_cache.insert(function);
        self.certainty = Certainty::Guarded;
        // A call site is allowed to become bounded polymorphic. Unlike the
        // old miss counter, a full cache does not permanently disable the
        // site: replacement keeps common call targets eligible while misses
        // still use the complete call gateway.
        QuickeningDecision::InstallCallGuard
    }

    /// Drop all physical state; the operation and fallback remain unchanged.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.callable_cache.clear();
        self.certainty = Certainty::Unknown;
        self.misses = 0;
        self.stable_hits = 0;
        self.tier = QuickeningTier::Cold;
    }

    fn record_miss(&mut self) {
        self.misses = self.misses.saturating_add(1);
        self.stable_hits = 0;
        self.tier = if self.misses >= MAX_MISSES {
            QuickeningTier::Megamorphic
        } else {
            QuickeningTier::Polymorphic
        };
    }

    fn stable_hit(&mut self) {
        self.stable_hits = self.stable_hits.saturating_add(1);
        if self.tier == QuickeningTier::Megamorphic && self.stable_hits >= MAX_MISSES {
            self.tier = QuickeningTier::Polymorphic;
            self.misses = 0;
        }
    }
}

fn mix_signature(mut hash: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

#[derive(Debug, Clone)]
struct CallableCache<const N: usize> {
    entries: [Option<std::rc::Weak<crate::value::FunctionValue>>; N],
    next_replacement: usize,
}

impl<const N: usize> PartialEq for CallableCache<N> {
    fn eq(&self, other: &Self) -> bool {
        self.next_replacement == other.next_replacement
            && self
                .entries
                .iter()
                .zip(other.entries.iter())
                .all(|(left, right)| match (left, right) {
                    (Some(left), Some(right)) => std::rc::Weak::ptr_eq(left, right),
                    (None, None) => true,
                    _ => false,
                })
    }
}

impl<const N: usize> CallableCache<N> {
    fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
            next_replacement: 0,
        }
    }

    fn lookup(&mut self, function: &std::rc::Rc<crate::value::FunctionValue>) -> bool {
        let mut hit = false;
        for entry in &mut self.entries {
            let Some(weak) = entry.as_ref() else {
                continue;
            };
            let Some(candidate) = weak.upgrade() else {
                // A dead weak edge is disposable physical state. Remove it
                // before probing so allocator address reuse cannot turn a
                // stale callable identity into a false hit.
                *entry = None;
                continue;
            };
            if std::rc::Rc::ptr_eq(&candidate, function) {
                hit = true;
            }
        }
        hit
    }

    fn insert(&mut self, function: &std::rc::Rc<crate::value::FunctionValue>) {
        let weak = std::rc::Rc::downgrade(function);
        if self.lookup(function) {
            return;
        }
        if N == 0 {
            return;
        }
        self.entries[self.next_replacement] = Some(weak);
        self.next_replacement = (self.next_replacement + 1) % N;
    }

    fn promote(&mut self, function: &std::rc::Rc<crate::value::FunctionValue>) {
        let Some(index) = self.entries.iter().position(|entry| {
            entry
                .as_ref()
                .and_then(std::rc::Weak::upgrade)
                .is_some_and(|entry| std::rc::Rc::ptr_eq(&entry, function))
        }) else {
            return;
        };
        if index != 0 {
            self.entries.swap(0, index);
        }
    }

    fn clear(&mut self) {
        self.entries.fill(None);
        self.next_replacement = 0;
    }

    fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GenericIcDecision, GenericInlineCache, QuickeningDecision, QuickeningSite, QuickeningTier,
    };
    use crate::facts::Certainty;
    use crate::ir::Opcode;
    use crate::shape_cache::{PropertyId, ShapeId};

    #[test]
    fn catalog_controls_quickening_eligibility() {
        let mut jump = QuickeningSite::<2>::new(Opcode::Jump);
        assert_eq!(
            jump.observe(ShapeId(1), PropertyId(1), 4),
            QuickeningDecision::Fallback
        );
        assert_eq!(jump.certainty(), Certainty::Unknown);

        let mut move_site = QuickeningSite::<2>::new(Opcode::Move);
        assert_eq!(
            move_site.observe(ShapeId(1), PropertyId(1), 4),
            QuickeningDecision::Fallback
        );
    }

    #[test]
    fn first_observation_installs_and_second_hits_guard() {
        let mut site = QuickeningSite::<2>::new(Opcode::GetProperty);
        assert_eq!(
            site.observe(ShapeId(1), PropertyId(2), 7),
            QuickeningDecision::InstallGuard { slot: 7 }
        );
        assert_eq!(
            site.observe(ShapeId(1), PropertyId(2), 99),
            QuickeningDecision::GuardedHit { slot: 7 }
        );
        assert_eq!(site.certainty(), Certainty::Guarded);
        assert_eq!(site.cache_len(), 1);
    }

    #[test]
    fn shape_probe_returns_cached_state_without_rederiving_it() {
        let mut site = QuickeningSite::<2>::new(Opcode::GetProperty);
        assert_eq!(
            site.observe(ShapeId(3), PropertyId(8), 41),
            QuickeningDecision::InstallGuard { slot: 41 }
        );
        assert_eq!(site.probe_shape(ShapeId(3), PropertyId(8)), Some(41));
        assert_eq!(site.probe_shape(ShapeId(3), PropertyId(9)), None);
    }

    #[test]
    fn callable_guard_installs_and_hits_by_identity() {
        let function = std::rc::Rc::new(crate::value::FunctionValue {
            code: crate::machine::FunctionCode::pending(Vec::new()),
            params: 0,
            captures: crate::environment::Environment::new(),
            with_captures: Vec::new(),
            properties: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            private_slots: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            private_environment: crate::private_environment::PrivateEnvironment::default(),
            instance_fields: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            kind: crate::ops::FunctionKind::Ordinary,
            strictness: crate::ops::FunctionStrictness::Sloppy,
            is_async: false,
            mapped_arguments: false,
        });
        let mut site = QuickeningSite::<2>::new(Opcode::Call);
        assert_eq!(
            site.observe_callable(&function),
            QuickeningDecision::InstallCallGuard
        );
        assert_eq!(
            site.observe_callable(&function),
            QuickeningDecision::GuardedCallHit
        );
        assert_eq!(site.callable_cache_len(), 1);
    }

    #[test]
    fn callable_cache_stays_bounded_and_rearms_after_replacement() {
        let make_function = || {
            std::rc::Rc::new(crate::value::FunctionValue {
                code: crate::machine::FunctionCode::pending(Vec::new()),
                params: 0,
                captures: crate::environment::Environment::new(),
                with_captures: Vec::new(),
                properties: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
                private_slots: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
                private_environment: crate::private_environment::PrivateEnvironment::default(),
                instance_fields: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
                kind: crate::ops::FunctionKind::Ordinary,
                strictness: crate::ops::FunctionStrictness::Sloppy,
                is_async: false,
                mapped_arguments: false,
            })
        };
        let functions = (0..3).map(|_| make_function()).collect::<Vec<_>>();
        let mut site = QuickeningSite::<2>::new(Opcode::Call);
        for function in &functions {
            assert_eq!(
                site.observe_callable(function),
                QuickeningDecision::InstallCallGuard
            );
        }
        assert_eq!(site.callable_cache_len(), 2);
        assert_eq!(
            site.observe_callable(&functions[2]),
            QuickeningDecision::GuardedCallHit
        );
    }

    #[test]
    fn misses_are_bounded_and_fall_back() {
        let mut site = QuickeningSite::<4>::new(Opcode::GetProperty);
        for shape in 1..=3 {
            assert!(matches!(
                site.observe(ShapeId(shape), PropertyId(1), shape as u32),
                QuickeningDecision::InstallGuard { .. }
            ));
        }
        assert!(matches!(
            site.observe(ShapeId(4), PropertyId(1), 4),
            QuickeningDecision::InstallGuard { .. }
        ));
        assert_eq!(site.misses(), 4);
        assert_eq!(site.certainty(), Certainty::Guarded);
        assert_eq!(site.tier(), QuickeningTier::Megamorphic);
        assert!(site.cache_len() <= 4);
    }

    #[test]
    fn invalidation_returns_to_unknown_without_semantic_reset() {
        let mut site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let shape = ShapeId(9);
        assert!(matches!(
            site.observe(shape, PropertyId(1), 3),
            QuickeningDecision::InstallGuard { .. }
        ));
        site.invalidate_shape(shape);
        assert_eq!(site.certainty(), Certainty::Unknown);
        assert_eq!(site.cache_len(), 0);
        assert_eq!(site.misses(), 1);
    }

    #[test]
    fn megamorphic_tier_rearms_after_stable_hits() {
        let mut site = QuickeningSite::<4>::new(Opcode::GetProperty);
        for shape in 1..=3 {
            assert!(matches!(
                site.observe(ShapeId(shape), PropertyId(1), shape),
                QuickeningDecision::InstallGuard { .. }
            ));
        }
        assert_eq!(site.tier(), QuickeningTier::Megamorphic);
        for _ in 0..3 {
            assert!(matches!(
                site.observe(ShapeId(3), PropertyId(1), 3),
                QuickeningDecision::GuardedHit { .. }
            ));
        }
        assert_eq!(site.tier(), QuickeningTier::Polymorphic);
    }

    #[test]
    fn generic_ic_runs_idempotent_phase_once_per_key_and_bounds_state() {
        let mut cache = GenericInlineCache::<u8, u16, 2>::new();
        let mut derives = 0;
        assert_eq!(
            cache.observe(1, |_| {
                derives += 1;
                Some(10)
            }),
            GenericIcDecision::Install(10)
        );
        assert_eq!(
            cache.observe(1, |_| {
                derives += 1;
                Some(99)
            }),
            GenericIcDecision::Hit(10)
        );
        assert_eq!(derives, 1);
        assert_eq!(
            cache.observe(2, |_| Some(20)),
            GenericIcDecision::Install(20)
        );
        assert_eq!(
            cache.observe(3, |_| Some(30)),
            GenericIcDecision::Install(30)
        );
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.lookup(&1), None);
        cache.clear();
        assert_eq!(cache.observe(4, |_| None), GenericIcDecision::Fallback);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn generic_ic_execute_shares_effect_and_complete_fallback() {
        let mut cache = GenericInlineCache::<u8, u16, 2>::new();
        let mut derives = 0;
        let first = cache.execute(
            "input-a",
            7,
            |_| {
                derives += 1;
                Some(70)
            },
            |input, state| format!("{input}:{state}"),
            |input| format!("fallback:{input}"),
        );
        assert_eq!(first, "input-a:70");
        let hit = cache.execute(
            "input-b",
            7,
            |_| {
                derives += 1;
                Some(99)
            },
            |input, state| format!("{input}:{state}"),
            |input| format!("fallback:{input}"),
        );
        assert_eq!(hit, "input-b:70");
        let fallback = cache.execute(
            "input-c",
            8,
            |_| None,
            |input, state| format!("{input}:{state}"),
            |input| format!("fallback:{input}"),
        );
        assert_eq!(fallback, "fallback:input-c");
        assert_eq!(derives, 1);
    }

    #[test]
    fn generic_ic_hit_promotes_without_changing_replacement_cursor() {
        let mut cache = GenericInlineCache::<u8, u16, 3>::new();
        cache.insert_state(1, 10);
        cache.insert_state(2, 20);
        cache.insert_state(3, 30);
        assert_eq!(cache.next_replacement, 0);

        assert_eq!(cache.observe(3, |_| Some(99)), GenericIcDecision::Hit(30));
        assert_eq!(
            cache.entries().map(|(key, _)| *key).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
        assert_eq!(cache.next_replacement, 2);

        assert_eq!(
            cache.observe(4, |_| Some(40)),
            GenericIcDecision::Install(40)
        );
        assert_eq!(cache.lookup(&1), None);
        assert_eq!(cache.lookup(&4), Some(40));
    }
}
