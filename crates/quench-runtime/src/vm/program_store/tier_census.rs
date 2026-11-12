//! The exit-time tier census (`QUENCH_TRACE=census`): per tier, how many units (functions,
//! regions, bodies) ran, how many times they were entered, and the bytecode instructions they
//! cover. Everything derives from counters the diagnostics already keep (`JsJitCounters`,
//! `LaneBodyStats`, `LoopSite`, the b-jit region totals); nothing is counted for it on a hot path.

use super::ProgramStore;

/// The tiers a function or loop can run in, in report order.
#[derive(Clone, Copy)]
enum Tier {
    InterpreterLane,
    InterpreterLoop,
    BjitRegion,
    UjitLaneBody,
    UjitOsrRegion,
    NumericFunction,
    NativeFunction,
}

impl Tier {
    const NAMES: [&'static str; Self::NativeFunction as usize + 1] = [
        "interpreter_lane",
        "interpreter_loop",
        "bjit_region",
        "ujit_lane_body",
        "ujit_osr_region",
        "numeric_function",
        "native_function",
    ];
}

/// One tier's totals. `instructions` is the static size of the units that were entered at least
/// once (a loop's span for its region), not a dynamic count.
#[derive(Clone, Copy, Default)]
struct Row {
    units: u64,
    entries: u64,
    instructions: u64,
}

#[derive(Default)]
pub(super) struct TierCensus([Row; Tier::NAMES.len()]);

impl TierCensus {
    /// A unit of `size` instructions that was entered `entries` times; never entered is not a unit.
    fn record(&mut self, tier: Tier, entries: u64, size: u64) {
        let row = &mut self.0[tier as usize];
        let ran = u64::from(entries > 0);
        row.units += ran;
        row.entries += entries;
        row.instructions += ran * size;
    }

    fn set(&mut self, tier: Tier, row: Row) {
        self.0[tier as usize] = row;
    }

    pub(super) fn render(&self) -> String {
        let tiers: Vec<String> = Tier::NAMES
            .iter()
            .zip(&self.0)
            .map(|(name, row)| {
                format!("\"{name}\":{{\"units\":{},\"entries\":{},\"instructions\":{}}}", row.units, row.entries, row.instructions)
            })
            .collect();
        format!("{{\"kind\":\"quench-tier-census\",\"instructions_are\":\"static-size-of-entered-units\",\"tiers\":{{{}}}}}", tiers.join(","))
    }
}

impl ProgramStore {
    /// `function`'s instruction count.
    fn function_size(&self, program: u32, function: u32) -> u64 {
        self.programs
            .get(program as usize)
            .and_then(Option::as_ref)
            .and_then(|entry| entry.residual.function_get(function as usize))
            .map_or(0, |body| body.instructions().count() as u64)
    }

    /// The tiers' totals so far; read once, at exit.
    pub(super) fn tier_census(&self) -> TierCensus {
        let mut census = TierCensus::default();
        for ((program, function), counters) in self.js_jit_diagnostics.iter().flatten() {
            let size = self.function_size(*program, *function);
            census.record(Tier::InterpreterLane, counters.lane_entries, size);
            census.record(Tier::NumericFunction, counters.numeric_entries, size);
            census.record(Tier::NativeFunction, counters.native_entries, size);
        }
        for (program, entry) in self.programs.iter().enumerate().filter_map(|(program, entry)| Some((program as u32, entry.as_ref()?))) {
            for function in (0..u32::MAX).map_while(|function| entry.residual.function_get(function as usize).map(|_| function)) {
                if let Some(view) = entry.lane_views.derived(function) {
                    census.record(Tier::UjitLaneBody, view.body_stats.runs(), self.function_size(program, function));
                    for site in view.osr_diagnostics().sites {
                        let span = u64::from(site.backedge_pc.saturating_sub(site.header_pc)) + 1;
                        census.record(Tier::InterpreterLoop, site.lane_backedge_count, span);
                        census.record(Tier::UjitOsrRegion, site.native_region_runs, span);
                    }
                }
            }
        }
        let (regions, instructions, entries) = super::super::compiled::base_region::census();
        census.set(Tier::BjitRegion, Row { units: regions, entries, instructions });
        census
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_counts_once_entered_and_its_size_with_it() {
        let mut census = TierCensus::default();
        census.record(Tier::UjitLaneBody, 5, 10);
        census.record(Tier::UjitLaneBody, 0, 99);
        census.record(Tier::UjitLaneBody, 1, 4);
        let row = census.0[Tier::UjitLaneBody as usize];
        assert_eq!((row.units, row.entries, row.instructions), (2, 6, 14));
    }

    #[test]
    fn render_names_every_tier() {
        let line = TierCensus::default().render();
        assert!(line.starts_with("{\"kind\":\"quench-tier-census\""));
        assert!(Tier::NAMES.iter().all(|name| line.contains(&format!("\"{name}\":{{\"units\":0"))));
    }
}
