//! The test engines load Node-API addons, so they export the executable's Node-API symbols
//! from the list `quench-node` links its own executable with.

fn main() {
    let list = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../quench-node/link/node-api.dynamic-list");
    println!("cargo:rerun-if-changed={}", list.display());
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let static_runtime = std::env::var("CARGO_CFG_TARGET_FEATURE")
        .is_ok_and(|features| features.split(',').any(|feature| feature == "crt-static"));
    if os == "linux" && !static_runtime {
        println!(
            "cargo:rustc-link-arg-bins=-Wl,--dynamic-list={}",
            list.display()
        );
    }
}
