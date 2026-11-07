//! The constant decodes the Cranelift front-ends still write by hand, which only go down.

/// Hand-written reads of a constant's immediate meaning in the Cranelift front-ends outside
/// this module: each is a second home of `immediate` or `constant_number`. The counts only go
/// down; a front-end reads constants through the facts.
#[test]
fn constant_decodes_outside_the_facts_only_shrink() {
    const DECODES: &[(&str, &str, usize)] = &[
        ("vm/js_compiled.rs", include_str!("../../js_compiled.rs"), 19),
        ("vm/compiled.rs", include_str!("../../compiled.rs"), 4),
        ("vm/compiled/osr_loop.rs", include_str!("../osr_loop.rs"), 0),
        ("vm/compiled/lane_region.rs", include_str!("../lane_region.rs"), 0),
        ("vm/compiled/lane_unit.rs", include_str!("../lane_unit.rs"), 0),
        ("vm/compiled/scalar_region.rs", include_str!("../scalar_region.rs"), 0),
        ("vm/compiled/static_loops.rs", include_str!("../static_loops.rs"), 0),
        ("vm/compiled/wavefront.rs", include_str!("../wavefront.rs"), 0),
    ];
    for (file, source, allowed) in DECODES {
        let found = source.matches("Constant::Number(").count();
        assert!(found <= *allowed, "{file} decodes constants {found} times (allowed {allowed}): use `facts::immediate`");
        assert!(found >= *allowed, "{file} now decodes constants {found} times: lower its allowance to {found}");
    }
}
