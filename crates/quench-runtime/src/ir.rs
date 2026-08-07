//! Quench's owned post-frontend representation.
//!
//! OXC allocations end with parsing. The parser lowers into this type before
//! the interpreter sees a program, so execution never borrows OXC AST nodes.
//! The first storage migration keeps the recursive statement nodes but packs
//! the top-level statement list as an owned slice.

pub use IrProgram as QuenchIr;

/// Owned runtime IR returned by the parser boundary.
pub struct IrProgram {
    statements: Box<[crate::ast::Statement]>,
}

impl IrProgram {
    pub(crate) fn from_program(program: crate::ast::Program) -> Self {
        let crate::ast::Program::Script(statements) = program;
        Self {
            statements: statements.into_boxed_slice(),
        }
    }

    pub(crate) fn statements(&self) -> &[crate::ast::Statement] {
        &self.statements
    }

    pub fn into_program(self) -> crate::ast::Program {
        crate::ast::Program::Script(self.statements.into_vec())
    }

    pub fn statement_count(&self) -> usize {
        self.statements.len()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parser_exposes_an_owned_ir_program_boundary() {
        let ir = crate::parser::parse_script_ir("1 + 2").expect("script should parse");
        assert_eq!(ir.statement_count(), 1);
    }
}
