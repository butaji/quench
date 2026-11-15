use super::*;

impl JitTierManager {
    /// Apply JavaScript's bounded statistical promotion transition to the
    /// manager-owned tier record. ProgramStore handles only global counters
    /// and diagnostic side effects after this decision is complete.
    pub(super) fn note_js_entry(
        &mut self,
        tiers: &Tiers,
        function: u32,
        numeric: bool,
        first_entry_candidate: bool,
        compiled_functions: usize,
        can_track: bool,
    ) -> JsEntryStep {
        if self.is_wasm() {
            return JsEntryStep::Count;
        }
        let full = compiled_functions >= JS_JIT_MAX_FUNCTIONS;
        let Some(state) = self.js_mut(function) else {
            if !can_track {
                return JsEntryStep::Count;
            }
            let (initial, step) = if full {
                (JsTierState::Unsupported, JsEntryStep::Count)
            } else if first_entry_candidate {
                (JsTierState::PromotionPending, JsEntryStep::PromoteAndCompile)
            } else {
                (JsTierState::Hot { observations: 1, numeric: u8::from(numeric) }, JsEntryStep::Count)
            };
            let inserted = self.js_insert(function, initial);
            debug_assert!(inserted);
            return step;
        };
        match state {
            JsTierState::Hot { observations, numeric: numeric_entries } => {
                *observations = observations.saturating_add(1);
                *numeric_entries = numeric_entries.saturating_add(u8::from(numeric));
                let promotion = numeric_promotion_decision(tiers, *numeric_entries, *observations);
                let (next, step) = if full {
                    (Some(JsTierState::Unsupported), JsEntryStep::Count)
                } else if first_entry_candidate || (promotion == Some(true) && numeric) {
                    (None, JsEntryStep::PromoteAndCompile)
                } else if promotion == Some(true) {
                    (Some(JsTierState::PromotionPending), JsEntryStep::Promote)
                } else {
                    let exhausted = promotion == Some(false) && *observations == u8::MAX;
                    (exhausted.then_some(JsTierState::Unsupported), JsEntryStep::Count)
                };
                if let Some(next) = next {
                    *state = next;
                }
                step
            }
            JsTierState::PromotionPending if full => {
                *state = JsTierState::Unsupported;
                JsEntryStep::Count
            }
            JsTierState::PromotionPending if numeric => JsEntryStep::Compile,
            JsTierState::PromotionPending
            | JsTierState::Rejected { .. }
            | JsTierState::Unsupported
            | JsTierState::Compiled(_) => JsEntryStep::Count,
        }
    }

    /// Advance the Wasm call-hotness state. Both the lane handoff and the
    /// ordinary dispatcher call this transition owner, so the threshold and
    /// one-shot attempt bit have one implementation.
    pub(super) fn note_wasm_entry(&mut self, tiers: &Tiers, function: u32, lane_handoff: bool) -> bool {
        let Some(state) = self.wasm_mut_or_default(function) else {
            return false;
        };
        let WasmTierState::Counting(count) = *state else {
            return false;
        };
        let count = count.saturating_add(1);
        if lane_handoff && count >= tiers.wasm.promotion_entries {
            return true;
        }
        if count < tiers.wasm.promotion_entries {
            *state = WasmTierState::Counting(count);
            return false;
        }
        *state = WasmTierState::Attempted;
        true
    }

    /// `note_wasm_tier_backedges`'s transition: one more observation, held below the
    /// threshold.
    pub(super) fn note_wasm_backedges(&mut self, tiers: &Tiers, function: u32) {
        let Some(state) = self.wasm_mut_or_default(function) else {
            return;
        };
        if let WasmTierState::Counting(count) = state {
            *count = count.saturating_add(1).min(tiers.wasm.promotion_entries - 1);
        }
    }

    pub(super) fn js(&self, function: u32) -> Option<&JsTierState> {
        match self {
            Self::JavaScript(functions) => functions.get(&function),
            Self::Empty | Self::Wasm(_) => None,
        }
    }

    pub(super) fn js_mut(&mut self, function: u32) -> Option<&mut JsTierState> {
        match self {
            Self::JavaScript(functions) => functions.get_mut(&function),
            Self::Empty | Self::Wasm(_) => None,
        }
    }

    pub(super) fn js_insert(&mut self, function: u32, state: JsTierState) -> bool {
        if matches!(self, Self::Empty) {
            *self = Self::JavaScript(FxHashMap::default());
        }
        match self {
            Self::JavaScript(functions) => {
                if let std::collections::hash_map::Entry::Vacant(slot) = functions.entry(function) {
                    slot.insert(state);
                    true
                } else {
                    false
                }
            }
            Self::Empty | Self::Wasm(_) => false,
        }
    }

    pub(super) fn js_values(&self) -> impl Iterator<Item = &JsTierState> {
        let functions = match self {
            Self::JavaScript(functions) => Some(functions),
            Self::Empty | Self::Wasm(_) => None,
        };
        functions.into_iter().flat_map(|functions| functions.values())
    }

    pub(super) fn wasm(&self, function: u32) -> Option<&WasmTierState> {
        match self {
            Self::Wasm(tiers) => tiers.functions.get(&function),
            Self::Empty | Self::JavaScript(_) => None,
        }
    }

    pub(super) fn wasm_mut_or_default(&mut self, function: u32) -> Option<&mut WasmTierState> {
        if matches!(self, Self::Empty) {
            *self = Self::Wasm(WasmTiers::default());
        }
        match self {
            Self::Wasm(tiers) => Some(tiers.functions.entry(function).or_default()),
            Self::Empty | Self::JavaScript(_) => None,
        }
    }

    /// The selective artifact holding `function` (`WasmTiers::holders`).
    pub(super) fn wasm_holding(&self, function: u32) -> Option<&Rc<super::compiled::CompiledProgram>> {
        let Self::Wasm(tiers) = self else { return None };
        tiers.functions.get(tiers.holders.get(&function)?)?.compiled()
    }

    pub(super) fn wasm_values(&self) -> impl Iterator<Item = &WasmTierState> {
        let functions = match self {
            Self::Wasm(tiers) => Some(&tiers.functions),
            Self::Empty | Self::JavaScript(_) => None,
        };
        functions.into_iter().flat_map(|functions| functions.values())
    }

    pub(super) fn wasm_tiers_mut(&mut self) -> Option<&mut WasmTiers> {
        if matches!(self, Self::Empty) {
            *self = Self::Wasm(WasmTiers::default());
        }
        match self {
            Self::Wasm(tiers) => Some(tiers),
            Self::Empty | Self::JavaScript(_) => None,
        }
    }

    /// Count `work` more entries of a selective module; whether `policy` says its
    /// whole-module compile is due (claimed by `claim_wasm_module_escalation`).
    pub(super) fn note_wasm_module_work(&mut self, work: u32, policy: WasmModuleCompile) -> bool {
        match self.wasm_tiers_mut().map(|tiers| &mut tiers.module) {
            Some(WasmModuleTier::Selective { work: observed, first }) => {
                let first = *first.get_or_insert_with(std::time::Instant::now);
                let before = *observed;
                *observed = observed.saturating_add(work);
                // A passed time bound counts as the work bound from then on, so every later
                // entry, whichever path observes it, finds the compile due.
                if *observed / WASM_JIT_MODULE_CLOCK_INTERVAL != before / WASM_JIT_MODULE_CLOCK_INTERVAL
                    && policy.due(0, first.elapsed())
                {
                    *observed = (*observed).max(policy.work);
                }
                policy.due(*observed, std::time::Duration::ZERO)
            }
            Some(WasmModuleTier::Whole | WasmModuleTier::Declined) | None => false,
        }
    }

    /// Whether the module's hot functions compile one by one (`WasmTierState`): under the
    /// selective A/B arm, which never compiles a module whole, or once a module's whole
    /// compile was claimed without an artifact (it failed, or the budget refused it). A module
    /// still waiting to compile whole runs interpreted until then: a short use (an npm
    /// package's one source-map parse) runs faster in the interpreter than through root
    /// compiles, and a long one would throw them away at its whole compile.
    pub(super) fn wasm_promotes_roots(&self, policy: WasmModuleCompile) -> bool {
        !self.wasm_module_declined() && (policy == WasmModuleCompile::NEVER || !self.wasm_module_selective())
    }

    pub(super) fn wasm_module_declined(&self) -> bool {
        matches!(self, Self::Wasm(tiers) if tiers.module == WasmModuleTier::Declined)
    }

    pub(super) fn wasm_module_selective(&self) -> bool {
        match self {
            Self::Wasm(tiers) => matches!(tiers.module, WasmModuleTier::Selective { .. }),
            Self::Empty => true,
            Self::JavaScript(_) => false,
        }
    }

    /// Claim the one whole-module compile of a selective module (`note_wasm_module_work`
    /// said it is due), and retire its selective artifacts: the module artifact holds every
    /// function they hold. Returns the retired artifacts' mapped bytes, or `None` when the
    /// module is not selective.
    pub(super) fn claim_wasm_module_escalation(&mut self) -> Option<(usize, usize)> {
        let tiers = match self {
            Self::Wasm(tiers) => tiers,
            Self::Empty | Self::JavaScript(_) => return None,
        };
        if matches!(tiers.module, WasmModuleTier::Whole | WasmModuleTier::Declined) {
            return None;
        }
        tiers.module = WasmModuleTier::Whole;
        tiers.holders.clear();
        let retired = tiers
            .functions
            .values_mut()
            .filter_map(WasmTierState::take_compiled)
            .fold((0, 0), |(count, bytes), compiled| (count + 1, bytes + compiled.mapped_bytes()));
        Some(retired)
    }

    pub(super) fn is_javascript(&self) -> bool {
        matches!(self, Self::JavaScript(_))
    }

    pub(super) fn is_wasm(&self) -> bool {
        matches!(self, Self::Wasm(_))
    }
}
