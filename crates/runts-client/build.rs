//! Build script for runts-client
//!
//! This script copies the TypeScript runtime to the dist directory.

use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let src_dir = Path::new(&manifest_dir).join("src");
    let dist_dir = Path::new(&manifest_dir).join("dist");
    
    // Create dist directory if it doesn't exist
    fs::create_dir_all(&dist_dir).unwrap();
    
    // Copy runtime.ts to dist/runtime.js
    // In a real build, this would be compiled/transpiled
    let runtime_src = src_dir.join("runtime.ts");
    let runtime_dst = dist_dir.join("runtime.js");
    
    if runtime_src.exists() {
        let content = fs::read_to_string(&runtime_src).unwrap();
        // Add a comment header
        let output = format!(
            "// Runts Client Runtime v{}\n// Built from: runtime.ts\n{}\n",
            env!("CARGO_PKG_VERSION"),
            content
        );
        fs::write(&runtime_dst, output).unwrap();
        println!("cargo:rerun-if-changed={}", runtime_src.display());
    }
    
    // Also copy to runts-lib so it can be served
    let lib_dir = Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("crates/runts-lib/src/runtime"))
        .unwrap_or_default();
    
    if lib_dir.exists() {
        let client_dir = lib_dir.join("client");
        fs::create_dir_all(&client_dir).ok();
        
        let runtime_dst2 = client_dir.join("runtime.js");
        if runtime_src.exists() {
            let content = fs::read_to_string(&runtime_src).unwrap();
            let output = format!(
                "// Runts Client Runtime v{}\n// Built from: runtime.ts\n{}\n",
                env!("CARGO_PKG_VERSION"),
                content
            );
            fs::write(&runtime_dst2, output).ok();
        }
    }
    
    println!("cargo:rustc-env=RUNTS_RUNTIME_VERSION={}", env!("CARGO_PKG_VERSION"));
}
