//! Interpreter-only build boundary.
//!
//! The former stencil declaration compiler generated guest machine-code
//! catalogs here. The legacy runtime is now an interpreter oracle, so its
//! compatibility selector is deliberately empty and contains no bytecode.

use std::{env, fs, path::PathBuf};

const REGION_KEYS: &[&str] = &[
    "add_chain_region_key",
    "add_const_region_key",
    "affine_i32_loop_region_key",
    "arithmetic_glue_region_key",
    "array_get_inc_number_region_key",
    "array_get_number_region_key",
    "array_loop_body_region_key",
    "array_numeric_fill_loop_region_key",
    "array_numeric_loop_region_key",
    "array_numeric_update_const_region_key",
    "array_numeric_update_region_key",
    "array_set_number_region_key",
    "binary_branch_glue_region_key",
    "binary_glue_region_key",
    "bitwise_and_region_key",
    "bitwise_not_region_key",
    "bitwise_or_region_key",
    "bitwise_shift_mask_return_region_key",
    "bitwise_xor_region_key",
    "bool_branch_region_key",
    "boolean_reduction_loop_region_key",
    "branch_glue_region_key",
    "branch_recurrence_loop_region_key",
    "call_region_key",
    "checked_store_glue_region_key",
    "compare_equal_region_key",
    "compare_equal_word_region_key",
    "compare_greater_equal_region_key",
    "compare_greater_region_key",
    "compare_less_branch_region_key",
    "compare_less_equal_region_key",
    "compare_less_region_key",
    "compare_not_equal_branch_region_key",
    "compare_not_equal_word_region_key",
    "conditional_f64_reduction_loop_region_key",
    "counted_continue_glue_region_key",
    "counted_decrement_glue_region_key",
    "counted_glue_region_key",
    "counted_i32_recurrence_region_key",
    "decrement_region_key",
    "dense_numeric_copy_loop_region_key",
    "dispatch_region_key",
    "fallthrough_region_key",
    "for_i_region_key",
    "get_index_region_key",
    "guarded_missing_property_return_region_key",
    "i32_counter_loop_region_key",
    "identity_region_key",
    "inc_glue_region_key",
    "increment_region_key",
    "load_const_region_key",
    "load_local_region_key",
    "loop_body_region_key",
    "loop_glue_region_key",
    "loop_region_key",
    "matrix_reduction_loop_region_key",
    "move_region_key",
    "multiply_region_key",
    "negate_region_key",
    "nested_branch_glue_region_key",
    "nested_xor_loop_region_key",
    "nullish_truthy_branch_return_region_key",
    "nullish_word_region_key",
    "number_classify_branch_return_region_key",
    "numeric_bitwise_loop_region_key",
    "numeric_floating_loop_region_key",
    "numeric_independent_loop_region_key",
    "numeric_integer_loop_region_key",
    "numeric_mixed_loop_region_key",
    "ordered_f64_reduction_loop_region_key",
    "parameter_glue_region_key",
    "property_region_key",
    "prototype_property_region_key",
    "return_word_region_key",
    "shift_left_region_key",
    "shift_right_region_key",
    "shift_right_zero_region_key",
    "store_glue_region_key",
    "store_local_region_key",
    "store_property_region_key",
    "switch_reduction_loop_region_key",
    "truthy_bool_branch_region_key",
    "truthy_number_region_key",
    "truthy_pointer_word_region_key",
    "truthy_word_region_key",
    "two_state_i32_loop_region_key",
    "typed_lane_loop_region_key",
    "unary_glue_region_key",
    "update_return_region_key",
    "word_const_fragment_region_key",
];

fn main() {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let mut catalog = String::from(concat!(
        "static NUMERIC_REGION_KEYS: &[(crate::ir::Opcode, crate::stencil_fact::RegionKey)] = &[];\n",
        "static CONTINUATION_REGION_KEYS: &[(crate::ir::Opcode, crate::stencil_fact::RegionKey)] = &[];\n",
        "static CANONICAL_REGION_TABLE: &[crate::stencil_select::RegionRecord] = &[];\n",
        "fn canonical_region_index(_: crate::stencil_fact::RegionKey) -> Option<usize> { None }\n",
        "fn canonical_region_lookup(_: crate::stencil_fact::RegionKey) -> Option<&'static crate::stencil_select::RegionRecord> { None }\n",
    ));
    for name in REGION_KEYS {
        catalog.push_str(&format!(
            "pub const fn {name}() -> crate::stencil_fact::RegionKey {{ crate::stencil_fact::RegionKey(0) }}\n"
        ));
    }
    catalog.push_str(concat!(
        "pub const fn fallthrough_region_id() -> crate::stencil_fact::RegionId { crate::stencil_fact::RegionId(0) }\n",
        "pub const fn loop_region_id() -> crate::stencil_fact::RegionId { crate::stencil_fact::RegionId(0) }\n",
        "pub fn binary_region_key(_: crate::ops::BinaryOp) -> Option<crate::stencil_fact::RegionKey> { None }\n",
        "pub fn binary_branch_region_key(_: crate::ops::BinaryOp) -> Option<crate::stencil_fact::RegionKey> { None }\n",
    ));
    fs::write(output.join("stencil_catalog.rs"), catalog).expect("write empty stencil catalog");
    fs::write(
        output.join("stencil_artifacts.rs"),
        "#[derive(Clone, Copy, Debug)] pub struct BuildStencilArtifact { pub name: &'static str, pub artifact_id: &'static str, pub key: crate::stencil_fact::RegionKey, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub abi: crate::stencil_select::RegionAbi, pub continuation_abi: crate::stencil_select::ContinuationAbi, pub entry: u16, pub external_entries: &'static [u16], pub has_fallthrough: bool, pub executable: bool, pub template_calls_helper: bool, pub data: &'static [u8], pub relocations: &'static [crate::stencil_select::PhysicalRelocation], pub links: &'static [crate::stencil_select::PhysicalLink], pub stencil: crate::stencil_fact::Stencil, pub fallthrough: Option<crate::stencil_fact::Stencil> }\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[];\nfn build_stencil_artifact_lookup(_: crate::stencil_fact::RegionKey) -> Option<&'static BuildStencilArtifact> { None }\n",
    )
    .expect("write empty stencil artifact table");
    println!("cargo:rustc-check-cfg=cfg(quench_production)");
    println!("cargo:rustc-check-cfg=cfg(quench_generated_stencil_artifacts)");
    println!("cargo:rerun-if-changed=build.rs");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    let lto = match profile.as_str() {
        "production" | "release" => "fat",
        "release-thin" => "thin",
        "debug" | "unknown" => "off",
        other => panic!("unsupported Cargo profile for quench runtime: {other}"),
    };
    if matches!(profile.as_str(), "release" | "production" | "release-thin") {
        println!("cargo:rustc-cfg=quench_production");
    }
    println!("cargo:rustc-env=QUENCH_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=QUENCH_BUILD_LTO={lto}");
    println!(
        "cargo:rustc-env=QUENCH_BUILD_TARGET={}",
        env::var("TARGET").unwrap_or_default()
    );
}
