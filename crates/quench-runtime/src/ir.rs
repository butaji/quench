//! Quench's owned post-frontend representation.
//!
//! OXC allocations end with parsing. The parser lowers into this type before
//! the interpreter sees a program, so execution never borrows OXC AST nodes.
//! The current representation reuses the compact runtime `Program` layout;
//! this named boundary allows its storage to evolve independently.

pub use crate::ast::Program as QuenchIr;

/// Owned runtime IR returned by the parser boundary.
pub struct IrProgram {
    program: crate::ast::Program,
}

impl IrProgram {
    pub(crate) fn from_program(program: crate::ast::Program) -> Self {
        Self { program }
    }

    pub fn as_program(&self) -> &crate::ast::Program {
        &self.program
    }

    pub fn into_program(self) -> crate::ast::Program {
        self.program
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parser_exposes_an_owned_ir_program_boundary() {
        let ir = crate::parser::parse_script_ir("1 + 2").expect("script should parse");
        assert!(matches!(ir.as_program(), crate::ast::Program::Script(_)));
    }
}
