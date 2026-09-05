//! Explicit lifecycle for an installed region stencil.

use crate::stencil_fact::{PatchValues, RegionKey};

const MAX_MISSES: u8 = crate::quickening::MAX_MISSES;
/// Maximum number of physical IC stubs retained at one rendered site.  The
/// chain is disposable metadata; exhaustion always returns to the complete
/// interpreter gateway rather than allocating a side table.
pub const MAX_IC_STUBS: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StencilState {
    Cold,
    Rendered,
    Installed,
    Repatch,
    Retired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactDelta {
    Same,
    HoleExpressible,
    RequiresRender,
    Degrade,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeResult {
    Hit,
    Miss,
    Retired,
}

/// The idempotent half of the probe/apply split.  A probe only reads facts.
pub trait IdempotentProbe {
    fn probe(&self, key: RegionKey) -> ProbeResult;
}

/// The effectful half.  Implementors may update disposable physical state;
/// semantic behavior remains owned by the ordinary interpreter.
pub trait EffectfulApply {
    fn apply(&mut self, key: RegionKey) -> Result<(), LifecycleError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleError {
    Retired,
    InvalidTransition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcStubPlacement {
    Inline,
    Outlined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IcStub {
    pub key: u64,
    pub address: usize,
    pub size: usize,
    pub placement: IcStubPlacement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcStubInstall {
    Installed(IcStub),
    Existing(IcStub),
    Fallback,
}

/// Fixed-capacity polymorphic IC chain for a rendered region.  Inline stubs
/// consume a reserved slab first; larger stubs are placed in the outlined
/// tail.  The chain never grows beyond `N`, and a miss after exhaustion is
/// represented explicitly so callers can invoke the ordinary interpreter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IcStubChain<const N: usize = MAX_IC_STUBS> {
    entries: [Option<IcStub>; N],
    len: usize,
    inline_start: usize,
    inline_used: usize,
    inline_capacity: usize,
    outlined_next: usize,
}

impl<const N: usize> IcStubChain<N> {
    pub const fn new(inline_start: usize, inline_capacity: usize, outlined_start: usize) -> Self {
        Self {
            entries: [None; N],
            len: 0,
            inline_start,
            inline_used: 0,
            inline_capacity,
            outlined_next: outlined_start,
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn capacity(&self) -> usize {
        N
    }

    pub const fn inline_used(&self) -> usize {
        self.inline_used
    }

    pub fn lookup(&self, key: u64) -> Option<IcStub> {
        self.entries[..self.len]
            .iter()
            .flatten()
            .find(|stub| stub.key == key)
            .copied()
    }

    pub fn install(&mut self, key: u64, size: usize) -> IcStubInstall {
        if let Some(stub) = self.lookup(key) {
            return IcStubInstall::Existing(stub);
        }
        if self.len == N {
            return IcStubInstall::Fallback;
        }
        let (address, placement) = if size <= self.inline_capacity.saturating_sub(self.inline_used)
        {
            let address = self.inline_start.saturating_add(self.inline_used);
            self.inline_used = self.inline_used.saturating_add(size);
            (address, IcStubPlacement::Inline)
        } else {
            let address = self.outlined_next;
            self.outlined_next = self.outlined_next.saturating_add(size);
            (address, IcStubPlacement::Outlined)
        };
        let stub = IcStub {
            key,
            address,
            size,
            placement,
        };
        self.entries[self.len] = Some(stub);
        self.len += 1;
        IcStubInstall::Installed(stub)
    }

    /// Walk the bounded chain in insertion order. `None` is the complete
    /// ordinary fallback path for a miss or an exhausted chain.
    pub fn dispatch(&self, key: u64) -> Option<IcStub> {
        self.lookup(key)
    }
}

/// Pure transition function used by both tests and the runtime state holder.
pub const fn transition(state: StencilState, delta: FactDelta) -> StencilState {
    match (state, delta) {
        (StencilState::Retired, _) => StencilState::Retired,
        (StencilState::Cold, FactDelta::Same) => StencilState::Cold,
        (StencilState::Cold, _) => StencilState::Rendered,
        (StencilState::Rendered, FactDelta::Same) => StencilState::Installed,
        (StencilState::Rendered, FactDelta::HoleExpressible) => StencilState::Installed,
        (StencilState::Rendered, FactDelta::Degrade) => StencilState::Retired,
        (StencilState::Rendered, _) => StencilState::Rendered,
        (StencilState::Installed, FactDelta::HoleExpressible) => StencilState::Repatch,
        (StencilState::Installed, FactDelta::Same) => StencilState::Installed,
        (StencilState::Installed, FactDelta::RequiresRender) => StencilState::Rendered,
        (StencilState::Installed, FactDelta::Degrade) => StencilState::Retired,
        (StencilState::Repatch, FactDelta::Same | FactDelta::HoleExpressible) => {
            StencilState::Installed
        }
        (StencilState::Repatch, FactDelta::RequiresRender) => StencilState::Rendered,
        (StencilState::Repatch, FactDelta::Degrade) => StencilState::Retired,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StencilLifecycle {
    state: StencilState,
    key: Option<RegionKey>,
    misses: u8,
}

impl Default for StencilLifecycle {
    fn default() -> Self {
        Self {
            state: StencilState::Cold,
            key: None,
            misses: 0,
        }
    }
}

impl StencilLifecycle {
    pub const fn new() -> Self {
        Self {
            state: StencilState::Cold,
            key: None,
            misses: 0,
        }
    }
    pub const fn state(&self) -> StencilState {
        self.state
    }
    pub const fn key(&self) -> Option<RegionKey> {
        self.key
    }
    pub const fn misses(&self) -> u8 {
        self.misses
    }

    pub fn observe(&mut self, key: RegionKey, hole_expressible: bool) -> StencilState {
        if self.state == StencilState::Retired {
            return self.state;
        }
        let delta = match self.key {
            None => FactDelta::RequiresRender,
            Some(previous) if previous == key => FactDelta::Same,
            Some(_) if hole_expressible => FactDelta::HoleExpressible,
            Some(_) => FactDelta::RequiresRender,
        };
        if matches!(delta, FactDelta::RequiresRender | FactDelta::Degrade) {
            self.misses = self.misses.saturating_add(1);
        }
        let delta = (self.misses >= MAX_MISSES)
            .then_some(FactDelta::Degrade)
            .unwrap_or(delta);
        self.state = transition(self.state, delta);
        if self.state != StencilState::Retired {
            self.key = Some(key);
        }
        self.state
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Permanently retire the installed physical version.  This is distinct
    /// from `reset`: reset starts a fresh admission history, while retirement
    /// is the fail-closed outcome for a committed physical failure.
    pub fn retire(&mut self) {
        self.state = StencilState::Retired;
    }

    /// Wire stencil admission to the interpreter site's existing bounded
    /// degrade tier. No independent miss threshold is introduced here.
    pub fn observe_site<const N: usize>(
        &mut self,
        site: &crate::quickening::QuickeningSite<N>,
        key: RegionKey,
        holes_cover_fact: bool,
    ) -> StencilState {
        if site.tier() == crate::quickening::QuickeningTier::Megamorphic {
            self.state = StencilState::Retired;
            return self.state;
        }
        self.observe(key, holes_cover_fact)
    }

    /// Complete a data-only repatch at a safepoint.  Restricting this effect to
    /// an explicit safepoint avoids torn reads by concurrently executing code;
    /// a code-byte change must instead go through `Rendered`.
    pub fn repatch_at_safepoint(
        &mut self,
        key: RegionKey,
        at_safepoint: bool,
    ) -> Result<StencilState, LifecycleError> {
        if !at_safepoint {
            return Err(LifecycleError::InvalidTransition);
        }
        match self.state {
            StencilState::Retired => return Err(LifecycleError::Retired),
            StencilState::Repatch => {}
            _ => return Err(LifecycleError::InvalidTransition),
        }
        self.key = Some(key);
        self.state = StencilState::Installed;
        Ok(self.state)
    }

    /// Apply the borrowed patch-data view before publishing the new installed
    /// key. The caller supplies the physical data slot update; instruction
    /// bytes are deliberately outside this transition. A failed update leaves
    /// the lifecycle and its previous key unchanged.
    pub fn repatch_values_at_safepoint<const N: usize>(
        &mut self,
        key: RegionKey,
        values: &PatchValues<'_, N>,
        at_safepoint: bool,
        apply_values: impl FnOnce(&PatchValues<'_, N>) -> Result<(), LifecycleError>,
    ) -> Result<StencilState, LifecycleError> {
        if !at_safepoint {
            return Err(LifecycleError::InvalidTransition);
        }
        match self.state {
            StencilState::Retired => return Err(LifecycleError::Retired),
            StencilState::Repatch => {}
            _ => return Err(LifecycleError::InvalidTransition),
        }
        apply_values(values)?;
        self.key = Some(key);
        self.state = StencilState::Installed;
        Ok(self.state)
    }
}

impl IdempotentProbe for StencilLifecycle {
    fn probe(&self, key: RegionKey) -> ProbeResult {
        if self.state == StencilState::Retired {
            ProbeResult::Retired
        } else if self.key == Some(key) {
            ProbeResult::Hit
        } else {
            ProbeResult::Miss
        }
    }
}

impl EffectfulApply for StencilLifecycle {
    fn apply(&mut self, key: RegionKey) -> Result<(), LifecycleError> {
        if self.state == StencilState::Retired {
            return Err(LifecycleError::Retired);
        }
        self.key = Some(key);
        self.state = StencilState::Installed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ic_stub_chain_walks_and_falls_back_at_bounded_ceiling() {
        let mut chain = IcStubChain::<3>::new(100, 24, 1000);
        for key in 1..=3 {
            assert!(matches!(
                chain.install(key, 8),
                IcStubInstall::Installed(IcStub {
                    placement: IcStubPlacement::Inline,
                    ..
                })
            ));
        }
        assert_eq!(chain.len(), 3);
        assert_eq!(chain.dispatch(1).map(|stub| stub.address), Some(100));
        assert_eq!(chain.dispatch(2).map(|stub| stub.address), Some(108));
        assert_eq!(chain.dispatch(3).map(|stub| stub.address), Some(116));
        assert_eq!(chain.dispatch(99), None);
        assert_eq!(chain.install(4, 8), IcStubInstall::Fallback);
        assert_eq!(chain.len(), chain.capacity());
    }

    #[test]
    fn ic_stub_chain_selects_inline_slab_or_outlined_stub() {
        let mut chain = IcStubChain::<4>::new(4_000, 16, 8_000);
        let inline = chain.install(1, 12);
        assert!(matches!(
            inline,
            IcStubInstall::Installed(IcStub {
                address: 4_000,
                placement: IcStubPlacement::Inline,
                ..
            })
        ));
        let outlined = chain.install(2, 12);
        assert!(matches!(
            outlined,
            IcStubInstall::Installed(IcStub {
                address: 8_000,
                placement: IcStubPlacement::Outlined,
                ..
            })
        ));
        assert_eq!(chain.inline_used(), 12);
    }

    #[test]
    fn ic_stub_chain_peak_state_is_bounded_under_repeated_keys() {
        let mut chain = IcStubChain::<MAX_IC_STUBS>::new(0, 32, 32);
        for round in 0..100 {
            for key in 0..(MAX_IC_STUBS as u64 + 1) {
                let _ = chain.install(round * 10 + key, 8);
            }
            assert_eq!(chain.len(), MAX_IC_STUBS);
        }
        assert!(chain.inline_used() <= 32);
    }

    #[test]
    fn lifecycle_prefers_data_repatch_before_render() {
        let key = RegionKey(1);
        let mut lifecycle = StencilLifecycle::new();
        assert_eq!(lifecycle.observe(key, false), StencilState::Rendered);
        assert_eq!(lifecycle.observe(key, false), StencilState::Installed);
        assert_eq!(lifecycle.observe(RegionKey(2), true), StencilState::Repatch);
        assert_eq!(
            lifecycle.observe(RegionKey(2), true),
            StencilState::Installed
        );
    }

    #[test]
    fn retirement_is_bounded_and_does_not_reenable() {
        let mut lifecycle = StencilLifecycle::new();
        for index in 0..3 {
            lifecycle.observe(RegionKey(index), false);
        }
        assert_eq!(lifecycle.state(), StencilState::Retired);
        assert_eq!(
            lifecycle.observe(RegionKey(99), true),
            StencilState::Retired
        );
        lifecycle.reset();
        assert_eq!(lifecycle.state(), StencilState::Cold);
    }

    #[test]
    fn explicit_retirement_is_not_a_rebuild_reset() {
        let mut lifecycle = StencilLifecycle::new();
        lifecycle.observe(RegionKey(1), false);
        lifecycle.retire();
        assert_eq!(lifecycle.state(), StencilState::Retired);
        assert_eq!(lifecycle.observe(RegionKey(2), true), StencilState::Retired);
        lifecycle.reset();
        assert_eq!(lifecycle.state(), StencilState::Cold);
    }

    #[test]
    fn patch_data_is_applied_before_installation_is_published() {
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let mut lifecycle = StencilLifecycle::new();
        let key = RegionKey(1);
        lifecycle.observe(key, false);
        lifecycle.observe(key, false);
        assert_eq!(lifecycle.observe(RegionKey(2), true), StencilState::Repatch);
        let mut observed_opcode = None;
        assert_eq!(
            lifecycle.repatch_values_at_safepoint(RegionKey(2), &values, true, |view| {
                observed_opcode = Some(view.opcode());
                Ok(())
            },),
            Ok(StencilState::Installed)
        );
        assert_eq!(observed_opcode, Some(crate::ir::Opcode::GetProperty));
        assert_eq!(lifecycle.key(), Some(RegionKey(2)));
    }

    #[test]
    fn repatch_effect_cannot_bypass_named_transition() {
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::Add);
        let values = PatchValues::from_site(&site);
        let mut lifecycle = StencilLifecycle::new();
        lifecycle.observe(RegionKey(1), false);
        lifecycle.observe(RegionKey(1), false);
        assert_eq!(lifecycle.state(), StencilState::Installed);
        assert_eq!(
            lifecycle.repatch_values_at_safepoint(RegionKey(2), &values, true, |_| Ok(())),
            Err(LifecycleError::InvalidTransition)
        );
        assert_eq!(lifecycle.key(), Some(RegionKey(1)));
    }
}
