//! The `QUENCH_TRACE=js-jit` report, printed as the store drops: the lane bodies' states, the
//! lane's miss reports, one JSON stats line per function of the diagnostics map, and the
//! store's totals.
use super::*;
use super::super::compiled::RegionRejection;
use super::super::dispatch_fast::{LaneView, OsrDiagnostics, OsrSiteDiagnostics, OsrSiteStatus};
use std::fmt::Display;
use std::io::{self, Write};

/// A JSON object's number members: each name with the value it prints.
type Members<'a> = &'a [(&'a str, &'a dyn Display)];

fn write_separator(out: &mut impl Write, index: usize) -> io::Result<()> {
    if index == 0 { Ok(()) } else { write!(out, ",") }
}

/// The members after a preceding one, each with its leading comma.
fn write_members(out: &mut impl Write, members: Members<'_>) -> io::Result<()> {
    members.iter().try_for_each(|(name, value)| write!(out, ",\"{name}\":{value}"))
}

/// An object of `members`, left open when `close` is false for the member that follows.
fn write_object(out: &mut impl Write, members: Members<'_>, close: bool) -> io::Result<()> {
    write!(out, "{{")?;
    for (index, (name, value)) in members.iter().enumerate() {
        write_separator(out, index)?;
        write!(out, "\"{name}\":{value}")?;
    }
    if close { write!(out, "}}") } else { Ok(()) }
}

/// The member `name`: an array of `items`, each written by `item`.
fn write_list<W: Write, T>(
    out: &mut W, name: &str, items: impl IntoIterator<Item = T>, mut item: impl FnMut(&mut W, T) -> io::Result<()>,
) -> io::Result<()> {
    write!(out, ",\"{name}\":[")?;
    for (index, entry) in items.into_iter().enumerate() {
        write_separator(out, index)?;
        item(out, entry)?;
    }
    write!(out, "]")
}

fn write_optional_string(out: &mut impl Write, value: Option<&str>) -> io::Result<()> {
    match value {
        Some(value) => write_json_string(out, value),
        None => write!(out, "null"),
    }
}

fn rejection_name(reason: RegionRejection) -> &'static str {
    match reason {
        RegionRejection::RegionTooLarge => "region-too-large",
        RegionRejection::UnsupportedInstruction(op) => crate::bytecode::opcode_name(usize::from(op)),
        RegionRejection::RegisterLimit => "register-limit",
        RegionRejection::CaptureLimit => "capture-limit",
        RegionRejection::ArrayLimit => "array-limit",
        RegionRejection::FieldLimit => "field-limit",
        RegionRejection::FieldStoreLimit => "field-store-limit",
        RegionRejection::FieldMutation => "field-mutation",
        RegionRejection::ArrayRedefined => "array-redefined",
    }
}

fn site_status_name(status: OsrSiteStatus) -> &'static str {
    match status {
        OsrSiteStatus::Admitted => "admitted",
        OsrSiteStatus::CallBoundary => "call-boundary",
        OsrSiteStatus::PromotionOnly => "promotion-only",
        OsrSiteStatus::Rejected => "rejected",
    }
}

/// How the function's tier stands: its state, or why it has none.
fn function_status(entry: Option<&ProgramEntry>, function: u32, counters: &JsJitCounters) -> &'static str {
    entry
        .and_then(|entry| entry.jit_tiers.js(function))
        .map(|tier| match tier {
            JsTierState::Hot { .. } => "hot",
            JsTierState::PromotionPending => "promotion-pending",
            JsTierState::Rejected { .. } | JsTierState::Unsupported => "unsupported",
            JsTierState::Compiled(_) => "compiled",
        })
        .unwrap_or(if counters.lane_entries != 0 { "lane-only" } else { "reclaimed" })
}

fn function_name<'a>(entry: Option<&'a ProgramEntry>, function: u32, counters: &'a JsJitCounters) -> &'a str {
    entry
        .and_then(|entry| entry.residual.functions.get(function as usize).map(|f| (entry, f)))
        .and_then(|(entry, function)| {
            let header = function.header();
            let atom = header.name.or(header.inferred_name)? as usize;
            (atom < entry.residual.atoms.len()).then(|| &entry.residual.atoms[atom])
        })
        .or(counters.function_name.as_deref())
        .unwrap_or("")
}

fn source_name<'a>(entry: Option<&'a ProgramEntry>, counters: &'a JsJitCounters) -> &'a str {
    entry
        .map(|entry| entry.residual.source_name.as_str())
        .or(counters.source_name.as_deref())
        .unwrap_or("")
}

/// The counters' own members: entries, caches, fast paths, the guard census and the ratios.
fn write_counters(out: &mut impl Write, counters: &JsJitCounters) -> io::Result<()> {
    let total_entries = counters.entries.saturating_add(counters.lane_entries);
    write_members(out, &[
        ("parameter_count", &counters.parameter_count), ("instruction_count", &counters.instruction_count),
        ("entries", &counters.entries), ("lane_entries", &counters.lane_entries),
        ("numeric_entries", &counters.numeric_entries), ("promotions", &counters.promotions),
        ("compile_attempts", &counters.compile_attempts), ("compile_successes", &counters.compile_successes),
        ("compile_ns", &counters.compile_ns), ("native_entries", &counters.native_entries),
        ("native_entry_coverage_ppm", &parts_per_million(counters.native_entries, counters.entries)),
        ("all_entry_native_coverage_ppm", &parts_per_million(counters.native_entries, total_entries)),
        ("field_cache_probes", &counters.field_cache_probes), ("field_cache_hits", &counters.field_cache_hits),
        ("field_store_cache_probes", &counters.field_store_cache_probes),
        ("field_store_cache_hits", &counters.field_store_cache_hits),
        ("field_return_candidate_entries", &counters.field_return_candidate_entries),
        ("field_return_fixed_this_entries", &counters.field_return_fixed_this_entries),
        ("field_return_fast_entries", &counters.field_return_fast_entries),
        ("field_increment_candidate_entries", &counters.field_increment_candidate_entries),
        ("field_increment_fixed_this_entries", &counters.field_increment_fixed_this_entries),
        ("field_increment_fast_entries", &counters.field_increment_fast_entries),
        ("captured_store_fast_entries", &counters.captured_store_fast_entries),
        ("numeric_find_fast_entries", &counters.numeric_find_fast_entries),
        ("fallback_entries", &counters.fallback_entries),
        ("interpreter_completions", &counters.interpreter_completions),
        ("interpreter_inclusive_ns", &counters.interpreter_inclusive_ns),
    ])?;
    write!(out, ",\"guard_census\":")?;
    write_object(out, &[
        ("conditional_branch_sites", &counters.guard_conditional_branches),
        ("typeof_operations", &counters.guard_typeof_ops),
        ("strict_comparison_sites", &counters.guard_strict_comparisons),
        ("relational_comparison_sites", &counters.guard_relational_comparisons),
    ], true)
}

/// The `osr` member: the diagnostics' totals and one object per loop site.
fn write_osr(out: &mut impl Write, osr: &OsrDiagnostics) -> io::Result<()> {
    write!(out, ",\"osr\":")?;
    write_object(out, &[
        ("plain_backward_jump_sites", &osr.plain_backward_jump_sites), ("admitted_sites", &osr.admitted_sites),
        ("promotion_only_sites", &osr.promotion_only_sites), ("compile_attempts", &osr.compile_attempts),
        ("compile_successes", &osr.compile_successes), ("currently_compiled_sites", &osr.currently_compiled_sites),
        ("native_entry_attempts", &osr.native_entry_attempts), ("native_region_runs", &osr.native_region_runs),
        ("tier_ups", &osr.tier_ups), ("body_runs", &osr.body_runs),
        ("entry_guard_misses", &osr.entry_guard_misses), ("exits", &osr.exits), ("widenings", &osr.widenings),
    ], false)?;
    write_list(out, "sites", &osr.sites, write_osr_site)?;
    write!(out, "}}")
}

fn write_osr_site(out: &mut impl Write, site: &OsrSiteDiagnostics) -> io::Result<()> {
    let status = format!("\"{}\"", site_status_name(site.status));
    write_object(out, &[
        ("header_pc", &site.header_pc), ("backedge_pc", &site.backedge_pc),
        ("status", &status),
        ("admitted", &site.admitted), ("deferred_call", &site.deferred_call),
        ("tier_threshold_reached", &site.tier_threshold_reached), ("lane_backedge_count", &site.lane_backedge_count),
    ], false)?;
    write_list(out, "call_boundary_pcs", &site.call_boundary_pcs, |out, pc| write!(out, "{pc}"))?;
    write!(out, ",\"failure_pc\":")?;
    match site.failure {
        Some(failure) => {
            write!(out, "{},\"failure_reason\":", failure.pc)?;
            write_json_string(out, rejection_name(failure.reason))?;
        }
        None => write!(out, "null,\"failure_reason\":null")?,
    }
    write_members(out, &[
        ("compile_attempts", &site.compile_attempts), ("compile_successes", &site.compile_successes),
        ("native_entry_attempts", &site.native_entry_attempts), ("native_region_runs", &site.native_region_runs),
        ("entry_guard_misses", &site.entry_guard_misses), ("exits", &site.exits), ("widenings", &site.widenings),
    ])?;
    write!(out, "}}")
}

/// An object of per-outcome counts, in the outcomes' order.
fn write_outcomes(out: &mut impl Write, counts: impl IntoIterator<Item = u64>) -> io::Result<()> {
    write!(out, "{{")?;
    for (index, (outcome, count)) in JsJitCallOutcome::ALL.into_iter().zip(counts).enumerate() {
        write_separator(out, index)?;
        write!(out, "\"{}\":{}", outcome.name(), count)?;
    }
    write!(out, "}}")
}

fn write_call_site(out: &mut impl Write, site: &JsJitCallSiteCounters) -> io::Result<()> {
    write!(out, "{{\"pc\":{},\"opcode\":", site.pc)?;
    write_json_string(out, crate::bytecode::opcode_name(usize::from(site.opcode)))?;
    write!(out, ",\"attempts\":{},\"direct_target\":", site.attempts)?;
    match site.direct_target {
        Some((program, function)) => write!(out, "{{\"program\":{program},\"function\":{function}}}")?,
        None => write!(out, "null")?,
    }
    write_members(out, &[
        ("direct_target_calls", &site.direct_target_calls), ("alternate_target_calls", &site.alternate_target_calls),
        ("other_target_calls", &site.other_target_calls), ("sampled_calls", &site.sampled_calls),
    ])?;
    write!(out, ",\"sampled_call_ns_by_outcome\":")?;
    write_outcomes(out, site.sampled_call_ns)?;
    write!(out, ",\"outcomes\":")?;
    write_outcomes(out, site.outcomes)?;
    write!(out, "}}")
}

/// The lists that close one function's stats line, each of its own counters.
fn write_tables(out: &mut impl Write, counters: &JsJitCounters, osr: &OsrDiagnostics) -> io::Result<()> {
    write_list(out, "call_sites", &counters.call_sites, write_call_site)?;
    let mut cache_sites: Vec<_> = counters.field_cache_sites.iter().collect();
    cache_sites.sort_unstable_by_key(|(site, _)| **site);
    write_list(out, "field_cache_sites", cache_sites, |out, (site, (probes, hits))| write!(
        out, "{{\"cache_site\":{site},\"probes\":{probes},\"hits\":{hits},\"hit_rate_ppm\":{}}}",
        parts_per_million(*hits, *probes)))?;
    write_list(out, "opcodes", counters.opcode_counts.iter(), |out, (opcode, count)| {
        write!(out, "[")?;
        write_json_string(out, crate::bytecode::opcode_name(usize::from(*opcode)))?;
        write!(out, ",{count}]")
    })?;
    write_list(out, "osr_analysis_rejections", &osr.analysis_rejections, |out, (reason, count)| {
        write!(out, "[")?;
        write_json_string(out, rejection_name(*reason))?;
        write!(out, ",{count}]")
    })
}

/// A lane body's report line, or its state where it has no stats.
fn report_lane_body(program: usize, function: u32, view: &LaneView, op_at: &dyn Fn(u32, u32) -> Option<String>) {
    if let Some(line) = view.body_stats.report(program, view.lane_body_state(), op_at) {
        eprintln!("{line}");
    } else {
        let (state, entries, claimed, warming) = view.lane_body_detail();
        eprintln!("QUENCH_LANE_STATE {{\"program\":{program},\"function\":{function},\"state\":\"{state}\",\"entries\":{},\"claimed\":{claimed},\"warming\":{warming},\"declined\":\"{}\"}}", entries.map_or(String::from("null"), |entries| entries.to_string()), view.body_stats.declined.get());
    }
}

impl ProgramStore {
    /// The `QUENCH_TRACE=js-jit` report, printed as the store drops.
    pub(super) fn report_js_jit(&self) {
        let Some(diagnostics) = &self.js_jit_diagnostics else {
            return;
        };
        self.report_lane_bodies();
        eprintln!("{}", super::super::dispatch_fast::lane_miss_report());
        eprintln!("{}", super::super::dispatch_fast::lane_unlowered_report());
        let mut records: Vec<_> = diagnostics.iter().collect();
        records.sort_unstable_by_key(|(key, _)| **key);
        let stderr = io::stderr();
        let mut stderr = stderr.lock();
        for ((program, function), counters) in records {
            let _ = self.write_function_stats(&mut stderr, (*program, *function), counters);
        }
        let _ = self.write_totals(&mut stderr);
    }

    fn report_lane_bodies(&self) {
        for (program, entry) in self.programs.iter().enumerate() {
            let Some(entry) = entry else { continue };
            let code: &ResidualProgram = &entry.residual;
            let op_at = |function: u32, pc: u32| code.function_get(function as usize)
                .and_then(|body| body.instructions().nth(pc as usize)).map(|instruction| format!("{:?}", instruction.op()));
            let views = (0..u32::MAX).map_while(|function| code.function_get(function as usize).map(|_| function))
                .filter_map(|function| entry.lane_views.derived(function).map(|view| (function, view)));
            for (function, view) in views {
                report_lane_body(program, function, view, &op_at);
            }
        }
    }

    /// One function's `QUENCH_JS_STATS` line.
    fn write_function_stats(&self, out: &mut impl Write, (program, function): (u32, u32), counters: &JsJitCounters)
        -> io::Result<()> {
        let entry = self.programs.get(program as usize).and_then(|entry| entry.as_deref());
        let osr = entry
            .and_then(|entry| entry.lane_views.derived(function))
            .map(LaneView::osr_diagnostics)
            .unwrap_or_default();
        write!(out, "QUENCH_JS_JIT_STATS {{\"program\":{program},\"function\":{function},\"function_name\":")?;
        write_json_string(out, function_name(entry, function, counters))?;
        write!(out, ",\"source_name\":")?;
        write_json_string(out, source_name(entry, counters))?;
        write!(out, ",\"rejection_reason\":")?;
        write_optional_string(out, counters.rejection_reason)?;
        write!(out, ",\"status\":\"{}\"", function_status(entry, function, counters))?;
        write_counters(out, counters)?;
        write_osr(out, &osr)?;
        write_tables(out, counters, &osr)?;
        writeln!(
            out,
            ",\"code_bytes\":{},\"guard_census_kind\":\"bytecode-sites-not-proven-redundancies\"}}",
            counters.code_bytes
        )
    }

    fn write_totals(&self, out: &mut impl Write) -> io::Result<()> {
        writeln!(
            out,
            "QUENCH_JS_JIT_DATAVIEW_FLOAT64 {{\"get_entries\":{},\"set_entries\":{}}}",
            self.js_jit_dataview_float64_get_entries,
            self.js_jit_dataview_float64_set_entries
        )?;
        writeln!(
            out,
            "QUENCH_JS_JIT_TIER_TRACKING {{\"tracked_functions\":{},\"tracking_limit\":{},\"exhaustion_events\":{}}}",
            self.js_jit_tracked_functions,
            JS_JIT_MAX_TRACKED_FUNCTIONS,
            self.js_jit_tracking_exhaustions
        )
    }
}
