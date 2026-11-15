//! Proved regions (`dispatch_fast::region_state`) against the general path, and their state machine.

use super::*;
use crate::test_host::TestHost;
use crate::vm::compiled::tier_policy::REGION_DECLINES;

fn run_lines(program: crate::ResidualProgram) -> Vec<String> {
    let host = TestHost::default();
    let mut vm = Vm::new(host.clone());
    vm.set_execution_mode(crate::ExecutionMode::Interpreted);
    vm.execute(&program).unwrap();
    host.lines().clone()
}

fn assert_matches_general(source: &str, name: &str) {
    let specialized = run_lines(crate::Engine::specialize(source, name).unwrap());
    let general = run_lines(crate::Engine::specialize_unspecialized(source, name).unwrap());
    assert!(!specialized.is_empty());
    assert_eq!(specialized, general);
}

#[test]
fn proved_int_regions_match_general_execution() {
    assert_matches_general(include_str!("../../tests/fixtures/proved-int-regions.js"), "proved-int-regions.js");
}

/// The proved regions of `function` after `source` ran: one `(proved, declines)` per region.
fn regions_after(source: &str, function: &str) -> Vec<(bool, u8)> {
    let program = crate::Engine::specialize(source, "region-state.js").unwrap();
    let index = program.functions.iter()
        .position(|candidate| candidate.header().name.is_some_and(|name| &program.atoms[name as usize] == function))
        .unwrap();
    let mut vm = Vm::new(TestHost::writing_to(Default::default()));
    vm.set_execution_mode(crate::ExecutionMode::Interpreted);
    vm.execute(&program).unwrap();
    let views = vm.programs.lane_views(ProgramId::MAIN).unwrap();
    let view = program_store::derived_lane_view(&views[index]).expect("the loop made the function's lane view");
    view._regions.iter().map(|region| region.state()).collect()
}

const COUNTED: &str = "function run(n, a) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; s = (s + a) | 0; } return s; }";

#[test]
fn an_int_loop_proves_its_region_at_entry() {
    let source = format!("{COUNTED} let r = 0; for (let k = 0; k < 40; k++) r = run(50, k); print(r);");
    assert_eq!(regions_after(&source, "run"), [(true, 0)]);
}

#[test]
fn a_failed_entry_demotes_and_the_region_retires_after_the_bound() {
    let source = format!("{COUNTED} let r = 0; for (let k = 0; k < 40; k++) r = run(50, 1); r = run(50, 0.5); print(r);");
    assert_eq!(regions_after(&source, "run"), [(false, 1)]);
    let alternating = format!("{COUNTED} let r = 0; for (let k = 0; k < 60; k++) r = run(20, k % 2 ? 0.5 : 1); print(r);");
    assert_eq!(regions_after(&alternating, "run"), [(false, REGION_DECLINES)]);
}

#[test]
fn a_loop_that_modifies_its_counter_has_no_region() {
    let source = "function run(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; i = i + 1; } return s; } \
        let r = 0; for (let k = 0; k < 40; k++) r = run(50); print(r);";
    assert!(regions_after(source, "run").is_empty());
}

#[test]
fn proved_field_regions_match_general_execution() {
    assert_matches_general(include_str!("../../tests/fixtures/proved-field-regions.js"), "proved-field-regions.js");
}

const POINT: &str = "function Pt(x, y) { this.x = x; this.y = y; } \
    function run(p, n) { let s = 0; for (let i = 0; i < n; i++) { p.x = (p.x + p.y) | 0; s = (s + p.x) | 0; } return s; }";

#[test]
fn a_packed_integer_field_loop_proves_its_region_and_a_double_declines_it() {
    let source = format!("{POINT} let r = 0; for (let k = 0; k < 40; k++) r = run(new Pt(1, 2), 30); print(r);");
    assert_eq!(regions_after(&source, "run"), [(true, 0)]);
    let double = format!("{POINT} let r = 0; for (let k = 0; k < 40; k++) r = run(new Pt(1, 2), 30); r = run(new Pt(1.5, 2), 30); print(r);");
    assert_eq!(regions_after(&double, "run"), [(false, 1)]);
}

#[test]
fn a_store_past_the_narrow_form_leaves_the_region_and_the_next_entry_proves_it_again() {
    let source = format!("{POINT} let r = 0; for (let k = 0; k < 40; k++) r = run(new Pt(1, 2), 30); \
        r = run(new Pt(1 << 29, 1 << 29), 30); r = run(new Pt(1, 2), 30); print(r);");
    assert_eq!(regions_after(&source, "run").len(), 1);
}
