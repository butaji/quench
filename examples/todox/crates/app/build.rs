//! Build script for app crate.
//!
//! Runs rune transpiler to generate Rust from .r.ts files.
//! Also creates a symlink to the generated code in the cache.

use std::env;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();
    
    // The generated code is in target/rune-cache/src/generated
    // relative to the workspace root
    let workspace_root = Path::new(&manifest_dir)
        .parent()  // crates/app
        .and_then(|p| p.parent())  // todox
        .unwrap();
    let cache_generated = workspace_root
        .join("target")
        .join("rune-cache")
        .join("src")
        .join("generated");
    
    // Create the out_dir/generated directory
    let gen_dest = Path::new(&out_dir).join("generated");
    std::fs::create_dir_all(&gen_dest).ok();
    
    // If cache exists, copy/symlink the generated code
    if cache_generated.exists() {
        // Copy all .rs files from cache to OUT_DIR
        if let Ok(entries) = std::fs::read_dir(&cache_generated) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "rs") {
                    let dest = gen_dest.join(path.file_name().unwrap());
                    std::fs::copy(&path, &dest).ok();
                }
            }
        }
        
        // Also copy subdirectories
        if let Ok(entries) = std::fs::read_dir(&cache_generated) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let dest = gen_dest.join(path.file_name().unwrap());
                    copy_dir_all(&path, &dest).ok();
                }
            }
        }
        
        // Create mod.rs for the generated directory
        let mod_rs = gen_dest.join("mod.rs");
        let mut content = String::new();
        
        // Protocol types re-exported for generated modules
        content.push_str("// Protocol types re-exported for generated modules\n");
        content.push_str("pub use protocol::{AppState, Filter, Task};\n\n");
        
        if let Ok(entries) = std::fs::read_dir(&gen_dest) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
                    if let Some(name) = path.file_stem() {
                        let name = name.to_string_lossy();
                        // Skip mod.rs itself
                        if name != "mod" {
                            content.push_str(&format!("pub mod {};\n", name));
                        }
                    }
                } else if path.is_dir() {
                    let name = path.file_name().unwrap().to_string_lossy();
                    content.push_str(&format!("pub mod {};\n", name));
                }
            }
        }
        std::fs::write(&mod_rs, &content).ok();
    } else {
        // No cache yet - create empty placeholder
        std::fs::write(gen_dest.join("mod.rs"), "// Placeholder\n").ok();
    }
    
    // Tell cargo about dependencies
    println!("cargo:rerun-if-changed=src/*.r.ts");
    println!("cargo:rerun-if-changed=src/**/*.r.ts");
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
