//! Explicit lifecycle for an installed region stencil.

use crate::stencil_fact::{PatchValues, RegionKey};

const MAX_MISSES: u8 = crate::quickening::MAX_MISSES;

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
