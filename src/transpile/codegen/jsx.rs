//! JSX generation

pub struct CodeGenJsx;

impl CodeGenJsx {
    pub fn jsx_to_rust(&mut self, _jsx: &crate::transpile::hir::JSXExpr) -> String {
        String::new()
    }
}
