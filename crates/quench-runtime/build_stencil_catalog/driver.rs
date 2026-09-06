fn main() {
    let declarations = region_declarations();
    generate_op_names();
    generate_stencil_catalog(&declarations);
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let target = env::var("TARGET").expect("TARGET for stencil catalog");
    println!("cargo:rustc-env=QUENCH_BUILD_TARGET={target}");
    build_stencil_artifacts::generate(&output, &declarations);
    validate_stencil_declarations(&declarations);
    if env::var_os("QUENCH_VERIFY_STENCIL_ENCODINGS").is_some() {
        verify_stencil_encodings();
    }
    println!("cargo:rustc-check-cfg=cfg(quench_production)");
    println!("cargo:rustc-check-cfg=cfg(quench_generated_stencil_artifacts)");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=QUENCH_VERIFY_STENCIL_ENCODINGS");
    println!("cargo:rerun-if-env-changed=QUENCH_GENERATE_STENCIL_OBJECTS");
    println!("cargo:rerun-if-env-changed=QUENCH_RUSTC");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_FEATURE");
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    // Keep this mapping exhaustive: a profile not represented here must not
    // silently masquerade as a production artifact.
    let lto = match profile.as_str() {
        "production" | "release" => "fat",
        "release-thin" => "thin",
        "debug" | "unknown" => "off",
        other => panic!("unsupported Cargo profile for quench runtime: {other}"),
    };
    let production = matches!(profile.as_str(), "release" | "production" | "release-thin");
    if production {
        println!("cargo:rustc-cfg=quench_production");
    }
    println!("cargo:rustc-env=QUENCH_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=QUENCH_BUILD_LTO={lto}");
}
