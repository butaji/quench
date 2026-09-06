// Rust-only build-time stencil artifact pipeline.
//
// The included units share one private representation. They are separated by
// responsibility so compiler/process effects stay at the edge while object
// verification and rendering remain deterministic and directly testable.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use object::read::{Object, ObjectSection, ObjectSymbol};
use object::SymbolSection;
use object::{
    BinaryFormat, RelocationEncoding, RelocationFlags, RelocationKind, RelocationTarget,
    SectionKind,
};

use super::RegionDeclaration;

const HEADER: &str = "/// Rust object artifacts generated at build time.\n";

pub(crate) fn generate(out_dir: &Path, declarations: &[RegionDeclaration]) {
    let target = env::var("TARGET").unwrap_or_default();
    let generation_enabled =
        env::var_os("QUENCH_GENERATE_STENCIL_OBJECTS").is_some() && supports_target(&target);
    if generation_enabled {
        println!("cargo:rustc-cfg=quench_generated_stencil_artifacts");
    }
    let generated = if generation_enabled {
        extract_objects(declarations)
    } else {
        empty_artifacts()
    };
    fs::write(out_dir.join("stencil_artifacts.rs"), generated)
        .expect("write generated stencil artifacts");
}

fn supports_target(target: &str) -> bool {
    target.starts_with("aarch64") || target.starts_with("x86_64")
}

include!("build_stencil_artifacts/model.rs");
include!("build_stencil_artifacts/public_verify.rs");
include!("build_stencil_artifacts/pipeline.rs");
include!("build_stencil_artifacts/compiler.rs");
include!("build_stencil_artifacts/relocation_contract.rs");
include!("build_stencil_artifacts/object_verify.rs");
include!("build_stencil_artifacts/render.rs");
include!("build_stencil_artifacts/process.rs");

#[cfg(test)]
include!("build_stencil_artifacts/tests.rs");
