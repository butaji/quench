//! Build script for app crate.
//!
//! Runs rune transpiler to generate Rust from .r.ts files.

fn main() {
    // In a full implementation, this would invoke the rune CLI
    // to transpile .r.ts files to .rs files in the generated directory

    // For now, we create placeholder files
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let gen_dir = format!("{}/generated", out_dir);
    std::fs::create_dir_all(&gen_dir).ok();

    // This would be replaced with actual rune transpilation:
    // let status = std::process::Command::new("cargo")
    //     .args(["rune", "transpile", "--out-dir", &gen_dir])
    //     .status()
    //     .expect("rune transpile failed");

    // Tell cargo to rerun if rune.toml changes
    println!("cargo:rerun-if-changed=rune.toml");
    println!("cargo:rerun-if-changed=src/*.r.ts");
    println!("cargo:rerun-if-changed=src/**/*.r.ts");
}
