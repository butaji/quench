use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=PROFILE");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    if matches!(profile.as_str(), "release" | "production" | "release-thin") {
        println!("cargo:rustc-cfg=quench_production");
    }
    println!("cargo:rustc-env=QUENCH_BUILD_PROFILE={profile}");
}
