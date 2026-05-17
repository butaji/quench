//! # App Build Script
//!
//! Runs the Rune compiler to generate Rust from .r.ts files.

fn main() {
    // In a full implementation, this would call the rune compiler
    println!("cargo:rerun-if-changed=src/*.r.ts");
    println!("cargo:rerun-if-changed=src/*.r.tsx");
    println!("cargo:rerun-if-changed=src/**/*.r.ts");
    println!("cargo:rerun-if-changed=src/**/*.r.tsx");
}
