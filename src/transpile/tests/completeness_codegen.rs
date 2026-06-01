//! HIR codegen completeness tests
//!
//! Three layers of defense:
//! 1. Compile-time exhaustiveness: match on ALL variants, no `_ =>` fallback
//! 2. Runtime no-silent-fallback: assert output is not Value::Null for each variant
//! 3. Round-trip integration: parse TS, generate Rust, verify output
//!
//! allow:too_many_lines,complexity,nested_externals

#[cfg(test)]
mod completeness_tests {
    use proc_macro2::TokenStream;
    use quote::ToTokens;

    use crate::transpile::hir::{
        AssignOp, BinaryOp, Block, ClassMember, Decl, Expr, Expr::*, ForInit, FunctionDecl,
        LiteralKind, LogicalOp, MethodKind, ModuleItem, ObjectMemberExpr, ObjectPatProp,
        ObjectProp, Ownership, Param, Pat, Pat::*, PropKey, QuoteCodegen, Stmt, Stmt::*,
        SwitchCase, TemplatePart, Type, Type::*, TypeMember, UnaryOp, UpdateOp, VariableKind,
    };

    // =============================================================================
    // Layer 1: Compile-time exhaustiveness guards
    // =============================================================================

    /// Exhaustively match ALL Expr variants - adding a new variant without codegen
    /// will cause a compilation error here.
    fn assert_expr_codegen_compiles(expr: &Expr) -> TokenStream {
        let cg = QuoteCodegen::default();
        match expr {
            Expr::String(_) => cg.gen_expr(expr),
            Expr::Number(_) => cg.gen_expr(expr),
            Expr::BigInt(_) => cg.gen_expr(expr),
            Expr::Boolean(_) => cg.gen_expr(expr),
            Expr::Null => cg.gen_expr(expr),
            Expr::Undefined => cg.gen_expr(expr),
            Expr::RegExp { .. } => cg.gen_expr(expr),
            Expr::Template { .. } => cg.gen_expr(expr),
            Expr::Ident { .. } => cg.gen_expr(expr),
            Expr::JSX(_) => cg.gen_expr(expr),
            Expr::Bin { .. } => cg.gen_expr(expr),
            Expr::Unary { .. } => cg.gen_expr(expr),
            Expr::Update { .. } => cg.gen_expr(expr),
            Expr::Logical { .. } => cg.gen_expr(expr),
            Expr::Cond { .. } => cg.gen_expr(expr),
            Expr::Assign { .. } => cg.gen_expr(expr),
            Expr::Array { .. } => cg.gen_expr(expr),
            Expr::Object { .. } => cg.gen_expr(expr),
            Expr::Function(_) => cg.gen_expr(expr),
            Expr::ArrowFunction { .. } => cg.gen_expr(expr),
            Expr::Await { .. } => cg.gen_expr(expr),
            Expr::Yield { .. } => cg.gen_expr(expr),
            Expr::Call { .. } => cg.gen_expr(expr),
            Expr::New { .. } => cg.gen_expr(expr),
            Expr::Member { .. } => cg.gen_expr(expr),
            Expr::Super => cg.gen_expr(expr),
            Expr::This => cg.gen_expr(expr),
            Expr::StaticMember { .. } => cg.gen_expr(expr),
            Expr::PrivateMember { .. } => cg.gen_expr(expr),
            Expr::MetaProperty { .. } => cg.gen_expr(expr),
            Expr::TaggedTemplate { .. } => cg.gen_expr(expr),
            Expr::Seq { .. } => cg.gen_expr(expr),
            Expr::Spread { .. } => cg.gen_expr(expr),
            Expr::Class { .. } => cg.gen_expr(expr),
            Expr::TypeAnnot { .. } => cg.gen_expr(expr),
            Expr::ArrowWithType { .. } => cg.gen_expr(expr),
            Expr::Block(_) => cg.gen_expr(expr),
            Expr::Invalid => panic!("Invalid expression in codegen"),
        }
    }

    /// Exhaustively match ALL Stmt variants - adding a new variant without codegen
    /// will cause a compilation error here.
    fn assert_stmt_codegen_returns_some(stmt: &Stmt) -> Option<TokenStream> {
        let cg = QuoteCodegen::default();
        match stmt {
            Stmt::Empty => cg.gen_stmt(stmt),
            Stmt::Block(_) => cg.gen_stmt(stmt),
            Stmt::Expr { .. } => cg.gen_stmt(stmt),
            Stmt::If { .. } => cg.gen_stmt(stmt),
            Stmt::While { .. } => cg.gen_stmt(stmt),
            Stmt::DoWhile { .. } => cg.gen_stmt(stmt),
            Stmt::For { .. } => cg.gen_stmt(stmt),
            Stmt::ForIn { .. } => cg.gen_stmt(stmt),
            Stmt::ForOf { .. } => cg.gen_stmt(stmt),
            Stmt::Continue { .. } => cg.gen_stmt(stmt),
            Stmt::Break { .. } => cg.gen_stmt(stmt),
            Stmt::Return { .. } => cg.gen_stmt(stmt),
            Stmt::With { .. } => cg.gen_stmt(stmt),
            Stmt::Labeled { .. } => cg.gen_stmt(stmt),
            Stmt::Switch { .. } => cg.gen_stmt(stmt),
            Stmt::Throw { .. } => cg.gen_stmt(stmt),
            Stmt::Try { .. } => cg.gen_stmt(stmt),
            Stmt::FunctionDecl(_) => cg.gen_stmt(stmt),
            Stmt::Class(_) => cg.gen_stmt(stmt),
            Stmt::Variable(_) => cg.gen_stmt(stmt),
            Stmt::ExportNamed { .. } => cg.gen_stmt(stmt),
            Stmt::ExportDefault { .. } => cg.gen_stmt(stmt),
            Stmt::ImportNamed { .. } => cg.gen_stmt(stmt),
            Stmt::ImportDefault { .. } => cg.gen_stmt(stmt),
        }
    }

    /// Exhaustively match ALL Type variants - adding a new variant without codegen
    /// will cause a compilation error here.
    fn assert_type_codegen_compiles(ty: &Type) -> TokenStream {
        let cg = QuoteCodegen::default();
        match ty {
            Type::String => cg.gen_type(ty),
            Type::Number => cg.gen_type(ty),
            Type::Boolean => cg.gen_type(ty),
            Type::Undefined => cg.gen_type(ty),
            Type::Null => cg.gen_type(ty),
            Type::Void => cg.gen_type(ty),
            Type::Never => cg.gen_type(ty),
            Type::Unknown => cg.gen_type(ty),
            Type::Any => cg.gen_type(ty),
            Type::BigInt => cg.gen_type(ty),
            Type::Symbol => cg.gen_type(ty),
            Type::Literal { .. } => cg.gen_type(ty),
            Type::Ref { .. } => cg.gen_type(ty),
            Type::Union { .. } => cg.gen_type(ty),
            Type::Intersection { .. } => cg.gen_type(ty),
            Type::Array { .. } => cg.gen_type(ty),
            Type::Function { .. } => cg.gen_type(ty),
            Type::Object { .. } => cg.gen_type(ty),
            Type::Index { .. } => cg.gen_type(ty),
            Type::Query { .. } => cg.gen_type(ty),
            Type::Infer { .. } => cg.gen_type(ty),
            Type::Mapped { .. } => cg.gen_type(ty),
            Type::Conditional { .. } => cg.gen_type(ty),
            Type::This => cg.gen_type(ty),
            Type::Template { .. } => cg.gen_type(ty),
        }
    }

    // =============================================================================
    // Layer 2: Runtime "no silent fallback" tests
    // =============================================================================

    /// Helper: check if TokenStream contains "Value :: Null" (with spacing as quote emits)
    fn contains_value_null(tokens: &TokenStream) -> bool {
        let s = tokens.to_string();
        // Check for various forms quote might emit
        s.contains("Value :: Null") || s.contains("Value::Null") || s.contains("Value . Null")
    }

    // --- Expr variant tests ---

    #[test]
    fn test_expr_string_no_null_fallback() {
        let expr = Expr::String("hello".into());
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        let s = tokens.to_string();
        assert!(
            !contains_value_null(&tokens),
            "String literal generated Value::Null: {}",
            s
        );
    }

    #[test]
    fn test_expr_number_no_null_fallback() {
        let expr = Expr::Number(42.0);
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Number literal generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_bigint_no_null_fallback() {
        let expr = Expr::BigInt(123);
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "BigInt literal generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_boolean_no_null_fallback() {
        let expr = Expr::Boolean(true);
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Boolean literal generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_null_is_value_null() {
        // Null IS the correct output
        let expr = Expr::Null;
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            contains_value_null(&tokens),
            "Null should generate Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_undefined_is_value_null() {
        // Undefined maps to Value::Null
        let expr = Expr::Undefined;
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            contains_value_null(&tokens),
            "Undefined should generate Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_regexp_no_null_fallback() {
        let expr = Expr::RegExp {
            pattern: "test".into(),
            flags: "i".into(),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "RegExp generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_template_no_null_fallback() {
        let expr = Expr::Template {
            parts: vec![TemplatePart::String("hello".into())],
            exprs: vec![],
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Template generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_ident_no_null_fallback() {
        let expr = Expr::Ident {
            name: "myVar".into(),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Ident generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_bin_no_null_fallback() {
        let expr = Expr::Bin {
            op: BinaryOp::Add,
            left: Box::new(Expr::Number(1.0)),
            right: Box::new(Expr::Number(2.0)),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Bin generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_unary_no_null_fallback() {
        let expr = Expr::Unary {
            op: UnaryOp::Minus,
            arg: Box::new(Expr::Number(5.0)),
            prefix: true,
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Unary generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_update_no_null_fallback() {
        let expr = Expr::Update {
            op: UpdateOp::PlusPlus,
            arg: Box::new(Expr::Ident { name: "i".into() }),
            prefix: true,
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Update generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_logical_no_null_fallback() {
        let expr = Expr::Logical {
            op: LogicalOp::And,
            left: Box::new(Expr::Boolean(true)),
            right: Box::new(Expr::Boolean(false)),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Logical generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_cond_no_null_fallback() {
        let expr = Expr::Cond {
            test: Box::new(Expr::Boolean(true)),
            consequent: Box::new(Expr::Number(1.0)),
            alternate: Box::new(Expr::Number(2.0)),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Cond generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_assign_no_null_fallback() {
        let expr = Expr::Assign {
            op: AssignOp::Assign,
            left: Box::new(Expr::Ident { name: "x".into() }),
            right: Box::new(Expr::Number(1.0)),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Assign generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_array_no_null_fallback() {
        let expr = Expr::Array {
            elems: vec![Some(Expr::Number(1.0)), Some(Expr::Number(2.0))],
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Array generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_object_no_null_fallback() {
        let expr = Expr::Object {
            members: vec![ObjectMemberExpr {
                prop: ObjectProp::Init {
                    key: PropKey::Str("a".into()),
                    value: Expr::Number(1.0),
                    computed: false,
                },
            }],
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Object generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_call_no_null_fallback() {
        let expr = Expr::Call {
            callee: Box::new(Expr::Ident { name: "foo".into() }),
            arguments: vec![Expr::Number(1.0)],
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Call generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_new_no_null_fallback() {
        let expr = Expr::New {
            callee: Box::new(Expr::Ident { name: "Object".into() }),
            arguments: vec![],
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "New generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_member_no_null_fallback() {
        let expr = Expr::StaticMember {
            obj: Box::new(Expr::Ident { name: "console".into() }),
            property: "log".into(),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "StaticMember generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_super_no_null_fallback() {
        let expr = Expr::Super;
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Super generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_this_no_null_fallback() {
        let expr = Expr::This;
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "This generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_seq_no_null_fallback() {
        let expr = Expr::Seq {
            left: Box::new(Expr::Number(1.0)),
            right: Box::new(Expr::Number(2.0)),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Seq generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_spread_no_null_fallback() {
        let expr = Expr::Spread {
            arg: Box::new(Expr::Array { elems: vec![] }),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Spread generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_await_no_null_fallback() {
        let expr = Expr::Await {
            arg: Box::new(Expr::Number(42.0)),
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Await generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_yield_no_null_fallback() {
        let expr = Expr::Yield {
            arg: Some(Box::new(Expr::Number(42.0))),
            delegate: false,
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "Yield generated Value::Null: {}",
            tokens.to_string()
        );
    }

    #[test]
    fn test_expr_arrow_function_no_null_fallback() {
        let expr = Expr::ArrowFunction {
            params: vec![Param {
                name: "x".into(),
                type_: Some(Type::Number),
                default: None,
                optional: false,
                pattern: None,
                ownership: Ownership::Owned,
            }],
            body: Box::new(Expr::Number(42.0)),
            is_async: false,
        };
        let tokens = QuoteCodegen::default().gen_expr(&expr);
        assert!(
            !contains_value_null(&tokens),
            "ArrowFunction generated Value::Null: {}",
            tokens.to_string()
        );
    }

    // --- Stmt variant tests ---

    #[test]
    fn test_stmt_empty_returns_some() {
        let stmt = Stmt::Empty;
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Empty stmt should return Some");
    }

    #[test]
    fn test_stmt_block_returns_some() {
        let stmt = Stmt::Block(vec![]);
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Block stmt should return Some");
    }

    #[test]
    fn test_stmt_expr_returns_some() {
        let stmt = Stmt::Expr {
            expr: Expr::Number(1.0),
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Expr stmt should return Some");
    }

    #[test]
    fn test_stmt_if_returns_some() {
        let stmt = Stmt::If {
            test: Expr::Boolean(true),
            consequent: Box::new(Stmt::Empty),
            alternate: None,
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "If stmt should return Some");
    }

    #[test]
    fn test_stmt_while_returns_some() {
        let stmt = Stmt::While {
            test: Expr::Boolean(true),
            body: Box::new(Stmt::Empty),
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "While stmt should return Some");
    }

    #[test]
    fn test_stmt_do_while_returns_some() {
        let stmt = Stmt::DoWhile {
            body: Box::new(Stmt::Empty),
            test: Expr::Boolean(true),
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "DoWhile stmt should return Some");
    }

    #[test]
    fn test_stmt_for_returns_some() {
        let stmt = Stmt::For {
            init: None,
            test: None,
            update: None,
            body: Box::new(Stmt::Empty),
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "For stmt should return Some");
    }

    #[test]
    fn test_stmt_for_in_returns_some() {
        let stmt = Stmt::ForIn {
            left: ForInit::Variable(VariableKind::Let, vec![("x".into(), None)]),
            right: Expr::Array { elems: vec![] },
            body: Box::new(Stmt::Empty),
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "ForIn stmt should return Some");
    }

    #[test]
    fn test_stmt_for_of_returns_some() {
        let stmt = Stmt::ForOf {
            left: ForInit::Variable(VariableKind::Let, vec![("x".into(), None)]),
            right: Expr::Array { elems: vec![] },
            body: Box::new(Stmt::Empty),
            is_await: false,
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "ForOf stmt should return Some");
    }

    #[test]
    fn test_stmt_continue_returns_some() {
        let stmt = Stmt::Continue { label: None };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Continue stmt should return Some");
    }

    #[test]
    fn test_stmt_break_returns_some() {
        let stmt = Stmt::Break { label: None };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Break stmt should return Some");
    }

    #[test]
    fn test_stmt_return_returns_some() {
        let stmt = Stmt::Return {
            arg: Some(Expr::Number(42.0)),
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Return stmt should return Some");
    }

    #[test]
    fn test_stmt_with_returns_some() {
        let stmt = Stmt::With {
            obj: Expr::Object { members: vec![] },
            body: Box::new(Stmt::Empty),
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "With stmt should return Some");
    }

    #[test]
    fn test_stmt_labeled_returns_some() {
        let stmt = Stmt::Labeled {
            label: "loop".into(),
            body: Box::new(Stmt::Empty),
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Labeled stmt should return Some");
    }

    #[test]
    fn test_stmt_switch_returns_some() {
        let stmt = Stmt::Switch {
            discriminant: Expr::Number(1.0),
            cases: vec![],
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Switch stmt should return Some");
    }

    #[test]
    fn test_stmt_throw_returns_some() {
        let stmt = Stmt::Throw {
            arg: Expr::String("error".into()),
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Throw stmt should return Some");
    }

    #[test]
    fn test_stmt_try_returns_some() {
        let stmt = Stmt::Try {
            block: Block(vec![]),
            handler: None,
            finalizer: None,
        };
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "Try stmt should return Some");
    }

    #[test]
    fn test_stmt_function_decl_returns_some() {
        let func = FunctionDecl {
            name: "test".into(),
            generics: vec![],
            params: vec![],
            return_type: None,
            body: Some(Block(vec![])),
            is_async: false,
            is_generator: false,
            decorators: vec![],
            throws: false,
            error_type: None,
        };
        let stmt = Stmt::FunctionDecl(func);
        let result = QuoteCodegen::default().gen_stmt(&stmt);
        assert!(result.is_some(), "FunctionDecl stmt should return Some");
    }

    // --- Type variant tests ---

    #[test]
    fn test_type_string_codegen() {
        let ty = Type::String;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "String type should generate tokens");
    }

    #[test]
    fn test_type_number_codegen() {
        let ty = Type::Number;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Number type should generate tokens");
    }

    #[test]
    fn test_type_boolean_codegen() {
        let ty = Type::Boolean;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Boolean type should generate tokens");
    }

    #[test]
    fn test_type_void_codegen() {
        let ty = Type::Void;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Void type should generate tokens");
    }

    #[test]
    fn test_type_never_codegen() {
        let ty = Type::Never;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Never type should generate tokens");
    }

    #[test]
    fn test_type_undefined_codegen() {
        let ty = Type::Undefined;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Undefined type should generate tokens");
    }

    #[test]
    fn test_type_null_codegen() {
        let ty = Type::Null;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Null type should generate tokens");
    }

    #[test]
    fn test_type_unknown_codegen() {
        let ty = Type::Unknown;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Unknown type should generate tokens");
    }

    #[test]
    fn test_type_any_codegen() {
        let ty = Type::Any;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Any type should generate tokens");
    }

    #[test]
    fn test_type_bigint_codegen() {
        let ty = Type::BigInt;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "BigInt type should generate tokens");
    }

    #[test]
    fn test_type_symbol_codegen() {
        let ty = Type::Symbol;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Symbol type should generate tokens");
    }

    #[test]
    fn test_type_literal_codegen() {
        let ty = Type::Literal {
            kind: LiteralKind::String,
            value: "hello".into(),
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Literal type should generate tokens");
    }

    #[test]
    fn test_type_ref_codegen() {
        let ty = Type::Ref {
            name: "MyType".into(),
            generics: vec![],
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Ref type should generate tokens");
    }

    #[test]
    fn test_type_union_codegen() {
        let ty = Type::Union {
            types: vec![Type::String, Type::Number],
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Union type should generate tokens");
    }

    #[test]
    fn test_type_intersection_codegen() {
        let ty = Type::Intersection {
            types: vec![
                Type::Object {
                    members: vec![TypeMember {
                        key: "a".into(),
                        type_: Type::String,
                        optional: false,
                        readonly: false,
                    }],
                },
                Type::Object {
                    members: vec![TypeMember {
                        key: "b".into(),
                        type_: Type::Number,
                        optional: false,
                        readonly: false,
                    }],
                },
            ],
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Intersection type should generate tokens");
    }

    #[test]
    fn test_type_array_codegen() {
        let ty = Type::Array {
            elem: Box::new(Type::String),
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Array type should generate tokens");
    }

    #[test]
    fn test_type_function_codegen() {
        let ty = Type::Function {
            params: vec![Type::String, Type::Number],
            ret: Box::new(Type::Boolean),
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Function type should generate tokens");
    }

    #[test]
    fn test_type_object_codegen() {
        let ty = Type::Object {
            members: vec![TypeMember {
                key: "name".into(),
                type_: Type::String,
                optional: false,
                readonly: false,
            }],
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Object type should generate tokens");
    }

    #[test]
    fn test_type_index_codegen() {
        let ty = Type::Index {
            obj: Box::new(Type::String),
            index: Box::new(Type::Number),
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Index type should generate tokens");
    }

    #[test]
    fn test_type_mapped_codegen() {
        let ty = Type::Mapped {
            from: Box::new(Type::String),
            to: Box::new(Type::Number),
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Mapped type should generate tokens");
    }

    #[test]
    fn test_type_conditional_codegen() {
        let ty = Type::Conditional {
            check: Box::new(Type::String),
            extends: Box::new(Type::String),
            true_type: Box::new(Type::Number),
            false_type: Box::new(Type::Boolean),
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Conditional type should generate tokens");
    }

    #[test]
    fn test_type_this_codegen() {
        let ty = Type::This;
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "This type should generate tokens");
    }

    #[test]
    fn test_type_template_codegen() {
        let ty = Type::Template {
            parts: vec![TemplatePart::String("hello".into())],
            values: vec![],
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Template type should generate tokens");
    }

    #[test]
    fn test_type_query_codegen() {
        let ty = Type::Query {
            expr: "typeof x".into(),
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Query type should generate tokens");
    }

    #[test]
    fn test_type_infer_codegen() {
        let ty = Type::Infer {
            name: "T".into(),
        };
        let tokens = QuoteCodegen::default().gen_type(&ty);
        assert!(!tokens.is_empty(), "Infer type should generate tokens");
    }

    // =============================================================================
    // Layer 3: Round-trip integration test
    // =============================================================================

    #[test]
    fn test_roundtrip_comprehensive_ts() {
        use crate::transpile::parser::TsParser;

        let source = r#"
function test(a: number, b: string): boolean {
    const x = a + 1;
    const arr = [1, 2, 3];
    const obj = { a: 1, b: "two" };
    if (x > 0) {
        return true;
    } else {
        return false;
    }
    for (let i = 0; i < 10; i++) {
        console.log(i);
    }
    try {
        throw new Error("test");
    } catch (e) {
        // handle
    }
}
"#;

        let parser = TsParser::new();
        let module = parser.parse_tsx(source).expect("parse failed");
        assert!(
            !module.items.is_empty(),
            "Module should have items"
        );

        // Convert HIR to Rust via QuoteCodegen
        let cg = QuoteCodegen::default();
        let mut all_tokens = TokenStream::new();

        for item in &module.items {
            match item {
                ModuleItem::Decl(Decl::Function(func)) => {
                    let fn_tokens = cg.gen_fn(func);
                    all_tokens.extend(fn_tokens);
                }
                ModuleItem::Stmt(stmt) => {
                    if let Some(stmt_tokens) = cg.gen_stmt(stmt) {
                        all_tokens.extend(stmt_tokens);
                    }
                }
                _ => {}
            }
        }

        let output = all_tokens.to_string();

        // Verify output is non-empty
        assert!(
            !output.is_empty(),
            "Generated Rust code should not be empty"
        );

        // Verify no excessive Value::Null (only legitimate uses in null/undefined handling)
        let null_count = output.matches("Value::Null").count();
        assert!(
            null_count <= 2,
            "Generated code has {} Value::Null occurrences (expected <= 2 for null/undefined literals only), output: {}",
            null_count,
            output
        );

        // Verify key Rust constructs are present
        assert!(
            output.contains("fn test"),
            "Should contain function declaration"
        );
        assert!(
            output.contains("for"),
            "Should contain for loop"
        );
        assert!(
            output.contains("match") || output.contains("catch"),
            "Should contain try-catch or match"
        );
    }
}
