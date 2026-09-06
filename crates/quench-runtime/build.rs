//! Build-time stencil catalog and Rust artifact generation boundary.
//!
//! The included units share this private module so declarations remain plain
//! data and every generated view is derived from the same representation.

use std::{env, fs, path::PathBuf, process::Command};

mod build_stencil_artifacts;
mod build_stencil_contract;
mod build_stencil_templates;

use build_stencil_contract::{
    rust_assembly_recipe, rust_leaf_recipe, DeclAbi, PhysicalBinding,
    PhysicalBindingValue, PhysicalOperand, PhysicalOperandField, RecipeComposition,
    RegionDeclaration, RustAssemblyRecipe, RustLeafRecipe,
};

include!("build_stencil_catalog/encoding_common.rs");
include!("build_stencil_catalog/encoding_x86.rs");
include!("build_stencil_catalog/encoding_aarch64.rs");
include!("build_stencil_catalog/declarations_leaf.rs");
include!("build_stencil_catalog/declarations_composed.rs");
include!("build_stencil_catalog/declarations_tagged.rs");
include!("build_stencil_catalog/driver.rs");
include!("build_stencil_catalog/encoding_verify.rs");
include!("build_stencil_catalog/catalog_render.rs");
include!("build_stencil_catalog/catalog_validate.rs");
