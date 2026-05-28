//! JavaScript code generator

pub mod expr;
pub mod jsx;
pub mod stmt;

pub fn generate_island_js(_name: &str, _module: &super::hir::Module) -> String {
    String::new()
}
