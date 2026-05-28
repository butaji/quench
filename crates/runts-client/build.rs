//! Build script for runts-client

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let dist_dir = std::path::Path::new(&manifest_dir).join("dist");
    std::fs::create_dir_all(&dist_dir).unwrap();
    let src = std::path::Path::new(&manifest_dir).join("src/runtime.ts");
    if src.exists() {
        let content = std::fs::read_to_string(&src).unwrap();
        let output = format!(
            "// Runts Client Runtime v{}\n{}\n",
            env!("CARGO_PKG_VERSION"),
            content
        );
        std::fs::write(dist_dir.join("runtime.js"), output).unwrap();
    }
}
