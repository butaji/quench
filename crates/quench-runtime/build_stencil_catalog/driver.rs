fn main() {
    let declarations = region_declarations();
    generate_op_names();
    generate_stencil_catalog(&declarations);
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let target = env::var("TARGET").expect("TARGET for stencil catalog");
    println!("cargo:rustc-env=QUENCH_BUILD_TARGET={target}");
    fs::write(
        output.join("stencil_artifacts.rs"),
        "#[derive(Clone, Copy, Debug)] pub struct BuildStencilArtifact { pub name: &'static str, pub artifact_id: &'static str, pub key: crate::stencil_fact::RegionKey, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub abi: crate::stencil_select::RegionAbi, pub continuation_abi: crate::stencil_select::ContinuationAbi, pub entry: u16, pub external_entries: &'static [u16], pub has_fallthrough: bool, pub executable: bool, pub template_calls_helper: bool, pub data: &'static [u8], pub relocations: &'static [crate::stencil_select::PhysicalRelocation], pub links: &'static [crate::stencil_select::PhysicalLink], pub stencil: crate::stencil_fact::Stencil, pub fallthrough: Option<crate::stencil_fact::Stencil> }\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[];\nfn build_stencil_artifact_lookup(_: crate::stencil_fact::RegionKey) -> Option<&'static BuildStencilArtifact> { None }\n",
    ).expect("write empty stencil artifact table");
    validate_stencil_declarations(&declarations);
    println!("cargo:rustc-check-cfg=cfg(quench_production)");
    println!("cargo:rustc-check-cfg=cfg(quench_generated_stencil_artifacts)");
    println!("cargo:rerun-if-env-changed=PROFILE");
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
