#[test]
fn test_property_helper_patch_is_scoped_to_property_helper_only() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test262_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/test262");
    let loader = super::HarnessLoader::new(&test262_dir.to_string_lossy());
    let source = "// sentinel: var x = Math.pow(2, 32) - 1; // end-sentinel\n";
    let script = loader
        .build_script(
            source,
            &[
                "assert.js".to_string(),
                "nativeErrors.js".to_string(),
                "propertyHelper.js".to_string(),
            ],
        )
        .expect("build_script should succeed");
    let sentinel_idx = script
        .find("// sentinel:")
        .expect("source must be appended verbatim to the built script");
    let source_section = &script[sentinel_idx..];
    assert!(
        source_section.contains("Math.pow(2, 32) - 1"),
        "test source containing the patched literal must be preserved verbatim, got: {source_section:?}"
    );
    let patched_marker = "var nonIndexNumericPropertyName = 999999;";
    assert!(
        script.contains(patched_marker),
        "propertyHelper.js must be patched in the built script, missing: {patched_marker}"
    );
    assert!(
        !script.contains("var nonIndexNumericPropertyName = Math.pow(2, 32) - 1;"),
        "the dangerous patch-target line must not appear in the built script"
    );
    let other_harness_section = script
        .split("function isConfigurable(")
        .next()
        .expect("propertyHelper.js section must be present in the built script");
    assert!(
        !other_harness_section.contains("Math.pow(2, 32) - 1"),
        "the patch must not bleed into other harness includes"
    );
}
