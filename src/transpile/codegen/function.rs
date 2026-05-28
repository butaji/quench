//! Function generation

pub struct FnGen;

impl FnGen {
    pub fn generate_function(
        _cg: &mut crate::transpile::codegen::CodeGenerator,
        _decl: &crate::transpile::hir::FunctionDecl,
        _is_component: bool,
    ) -> anyhow::Result<String> {
        Ok(String::new())
    }
}
