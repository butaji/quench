//! Build-time stencil catalog and Rust artifact generation boundary.
//!
//! The included units share this private module so declarations remain plain
//! data and every generated view is derived from the same representation.

use std::{env, fs, path::PathBuf, process::Command};

mod build_stencil_artifacts;
#[macro_use]
mod build_stencil_contract;
mod build_stencil_templates;

use build_stencil_contract::{
    equal, operand, region_key_name, value, AssemblyContinuation, AssemblyControlLink,
    AssemblySuccessor, AssemblySuccessorRole, DeclAbi, DeclContinuationAbi, PhysicalBinding,
    PhysicalBindingValue, PhysicalOperand, PhysicalOperandField, PhysicalOutput,
    PhysicalOutputDestination, PhysicalOutputValue, RecipeComposition, RegionDeclaration,
};

include!("build_stencil_catalog/encoding_common.rs");
include!("build_stencil_catalog/encoding_x86.rs");
include!("build_stencil_catalog/encoding_aarch64.rs");
include!("build_stencil_catalog/declarations_rust_leaf.rs");
include!("build_stencil_catalog/declarations_rust_assembly.rs");
include!("build_stencil_catalog/declarations_composed.rs");
include!("build_stencil_catalog/declarations_tagged.rs");
include!("build_stencil_catalog/driver.rs");
include!("build_stencil_catalog/encoding_verify.rs");
include!("build_stencil_catalog/catalog_physical.rs");
include!("build_stencil_catalog/catalog_keys.rs");
include!("build_stencil_catalog/catalog_links.rs");
include!("build_stencil_catalog/catalog_render.rs");
include!("build_stencil_catalog/catalog_validate.rs");
