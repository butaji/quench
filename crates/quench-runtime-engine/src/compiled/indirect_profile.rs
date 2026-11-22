//! The profile of an artifact's indirect call sites: how often the VM's callout resolved each
//! target function at each site (data, by function index). A target a site's table still
//! holds, read from the table's mirror rows, is what a selective compile may promote beside its
//! caller (`profiled_indirect_targets`): the rows say which function is where, so no per-site
//! cache keeps a second copy of it.

use super::{CompiledProgram, MAX_JIT_PROFILED_INDIRECT_TARGETS, ProgramId, Vm};
use quench_runtime::Value;
use quench_runtime::heap::{Cell, TableTarget};
use std::cell::RefCell;

/// A target function and how often the callout resolved it at a site.
#[derive(Clone, Copy)]
struct TargetHits {
    function: u32,
    hits: u64,
}

/// Per call site, the targets resolved there, ordered by function index.
pub(super) struct SiteHits(RefCell<Vec<Vec<TargetHits>>>);

impl SiteHits {
    pub(super) fn new(sites: usize) -> Self {
        Self(RefCell::new(vec![Vec::new(); sites]))
    }

    fn note(&self, site: usize, function: u32) {
        let mut sites = self.0.borrow_mut();
        let Some(targets) = sites.get_mut(site) else {
            return;
        };
        match targets.binary_search_by_key(&function, |target| target.function) {
            Ok(at) => targets[at].hits = targets[at].hits.saturating_add(1),
            Err(at) => targets.insert(at, TargetHits { function, hits: 1 }),
        }
    }
}

/// The rows of the table bound at environment slot `slot` of `env`.
fn table_rows(vm: &Vm, env: Value, slot: u32) -> Option<&[TableTarget]> {
    let binding = vm.heap.environment_slot(env, slot as usize)?;
    match vm.heap.get(binding)? {
        Cell::WasmTable { elements, .. } => Some(elements.targets()),
        _ => None,
    }
}

#[quench_macros::ext(CompiledProgramOps2)]
impl CompiledProgram {
    /// Counts a resolution of `function` at the call site `site`.
    pub(super) fn note_indirect_target(&self, site: usize, function: u32) {
        self.indirect_hits.note(site, function);
    }

    /// The targets resolved at call sites of a selective root's static closure that their
    /// tables still hold for the instance `env`, hottest first.
    pub(crate) fn profiled_indirect_targets(
        &self,
        vm: &Vm,
        program: ProgramId,
        env: Value,
        callers: &[bool],
    ) -> Vec<usize> {
        let Some(instance) = vm.wasm_instance_address(env, program) else {
            return Vec::new();
        };
        let sites = self.indirect_hits.0.borrow();
        let mut observed: Vec<TargetHits> = Vec::new();
        for ((caller, _, table), targets) in self.reference_cache_sites.iter().zip(sites.iter()) {
            if !callers.get(*caller).copied().unwrap_or(false) {
                continue;
            }
            let Some(rows) = table_rows(vm, env, *table) else {
                continue;
            };
            let held = |target: &&TargetHits| {
                rows.iter()
                    .any(|row| row.entry == target.function && row.instance == instance as u64)
            };
            observed.extend(targets.iter().filter(held).copied());
        }
        observed.sort_unstable_by_key(|target| std::cmp::Reverse(target.hits));
        let mut unique = Vec::new();
        for target in observed {
            let function = target.function as usize;
            if !unique.contains(&function) {
                unique.push(function);
                if unique.len() == MAX_JIT_PROFILED_INDIRECT_TARGETS {
                    break;
                }
            }
        }
        unique
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_accumulate_per_site_and_target_in_function_order() {
        let hits = SiteHits::new(2);
        for function in [5, 3, 5, 5, 3, 9] {
            hits.note(0, function);
        }
        hits.note(1, 3);
        hits.note(7, 1);
        let sites = hits.0.borrow();
        let counts = |site: usize| sites[site].iter().map(|t| (t.function, t.hits)).collect::<Vec<_>>();
        assert_eq!(counts(0), [(3, 2), (5, 3), (9, 1)]);
        assert_eq!(counts(1), [(3, 1)]);
    }
}
