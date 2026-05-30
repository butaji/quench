#[cfg(test)]
mod codegen_tests {
    use crate::transpile::codegen::CodeGenerator;
    use crate::transpile::hir::*;

    fn create_codegen() -> CodeGenerator {
        CodeGenerator::new()
    }

    #[test]
    fn test_generate_interface_to_struct() {
        let cg = create_codegen();
        let decl = TypeDecl {
            name: "CounterProps".to_string(),
            generics: vec![],
            type_: Type::Object { members: vec![] },
        };
        let result = cg.generate_type_decl(&decl);
        assert!(result.is_ok());
    }

    #[test]
    fn test_snake_case() {
        let cg = create_codegen();
        assert_eq!(cg.to_snake_case("useState"), "use_state");
        assert_eq!(cg.to_snake_case("onClick"), "on_click");
    }
}
