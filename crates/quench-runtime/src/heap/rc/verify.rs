//! The counting verifier (`QUENCH_DEV=rc-verify`): after a full collection's mark, reports
//! each kept suspect the trace reached, with the live holders and cells that name it.

use super::*;

/// Reachable suspects printed by `rc_check_suspects`.
const RC_VERIFY_SAMPLE: usize = 4;
/// Holders named by reachable suspects printed, in all.
const RC_VERIFY_HOLDERS: usize = 8;
/// Holders and cells named by reachable suspects printed, in all.
const RC_VERIFY_CELLS: usize = 16;

impl Heap {
    /// Reports, after a full collection's mark, each kept suspect (`rc_suspects`) the trace
    /// reached, which counts would have freed while reachable, with the live holders that
    /// name it: a count or a pin that missed an edge.
    pub(in crate::heap) fn rc_check_suspects(&mut self) {
        let mut suspects = std::mem::take(&mut self.rc_suspects);
        suspects.sort_unstable_by_key(|value| value.to_bits());
        suspects.dedup();
        let reached: rustc_hash::FxHashSet<Value> =
            suspects.iter().copied().filter(|&value| self.rc_is_old(value)).collect();
        eprintln!("rc-verify: {} suspects, {} reachable", suspects.len(), reached.len());
        if reached.is_empty() {
            return;
        }
        for value in reached.iter().take(RC_VERIFY_SAMPLE) {
            eprintln!("rc-verify: reachable suspect {value:?} count {:?}", self.rc_count_of(*value));
        }
        let mut reported = 0;
        self.report_holders(&reached, &mut reported);
        self.report_cells(&reached, &mut reported);
        if self.roots.values().any(|value| reached.contains(&value)) {
            eprintln!("rc-verify: the root table names a reachable suspect");
        }
    }

    /// Reports the old records that name a reached suspect, up to `RC_VERIFY_HOLDERS` in all.
    fn report_holders(&self, reached: &rustc_hash::FxHashSet<Value>, reported: &mut usize) {
        let mut children = Vec::new();
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let holder = handle(id);
                if !self.rc_is_old(holder) {
                    return;
                }
                self.rc_children(holder, &mut children);
                for child in children.drain(..) {
                    if reached.contains(&child) && *reported < RC_VERIFY_HOLDERS {
                        *reported += 1;
                        eprintln!("rc-verify: record {holder:?} (count {:?}) names {child:?}", self.rc_count_of(holder));
                    }
                }
            });
        }
    }

    /// Reports the legacy cells that name a reached suspect, up to `RC_VERIFY_CELLS` in all.
    fn report_cells(&self, reached: &rustc_hash::FxHashSet<Value>, reported: &mut usize) {
        let mut children = Vec::new();
        let mut last_proto = PushedProtos::default();
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(cell) = &slot.cell else { continue };
            Self::children(Value::heap(index as u32), cell, &self.properties, &mut children, &mut |_, _, _, _, _| {}, GcTraceKind::Collection, &mut last_proto);
            for child in children.drain(..) {
                if reached.contains(&child) && *reported < RC_VERIFY_CELLS {
                    *reported += 1;
                    eprintln!("rc-verify: cell {index} ({:?}) names {child:?}", std::mem::discriminant(cell));
                }
            }
        }
    }
}

impl Heap {
    /// The records whose count differs from the references the live records' contents hold
    /// with the count and the references: none when counting is right.
    #[cfg(test)]
    pub(in crate::heap) fn rc_mismatches(&self) -> Vec<(Value, u32, usize)> {
        let mut references: std::collections::HashMap<Value, usize> = std::collections::HashMap::new();
        let mut children = Vec::new();
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |id| {
                self.rc_children(handle(id), &mut children);
                for child in children.drain(..) {
                    if locate(child).is_some() {
                        *references.entry(child).or_default() += 1;
                    }
                }
            });
        }
        let mut mismatches = Vec::new();
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let value = handle(id);
                let (family, id) = locate(value).expect("a record");
                let count = with_space!(self, family, |space| space.rc_exact_count(id));
                let held = references.get(&value).copied().unwrap_or(0);
                if count != u32::MAX && count as usize != held {
                    mismatches.push((value, count, held));
                }
            });
        }
        mismatches
    }
}
