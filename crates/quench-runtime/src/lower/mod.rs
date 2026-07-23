//! Lower OXC AST to runtime AST
//!
//! Converts oxc::ast nodes to our runtime AST representation.

pub mod control_flow;
pub mod expr;
pub mod helpers;
pub mod jsx;
pub mod literals;
pub mod opt_chain;
pub mod pattern;
pub mod stmt;

pub use expr::lower_expr;
pub use helpers::{wtf8_atom_to_string as atom_to_string, wtf8_atom_to_string, LowerError};
pub use stmt::{lower_module, lower_program, lower_script, lower_stmt};

#[cfg(test)]
mod tests {
    use crate::ast::{Expression, Program, Statement};
    use crate::lower::{lower_module, lower_script};
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    fn parse_and_lower_script(source: &str) -> Program {
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, source_type).parse();
        lower_script(&ret.program).unwrap()
    }

    fn parse_and_lower_module(source: &str) -> Program {
        let source_type = SourceType::default().with_module(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, source_type).parse();
        lower_module(&ret.program).unwrap()
    }

    fn first_stmt(source: &str) -> Statement {
        match parse_and_lower_script(source) {
            Program::Script(stmts) => stmts.into_iter().next().unwrap(),
        }
    }

    // ===== Script/Module Lowering =====

    #[test]
    fn test_lower_script_basic() {
        let prog = parse_and_lower_script("var x = 1;");
        assert!(matches!(prog, Program::Script(_)));
    }

    #[test]
    fn test_lower_module_basic() {
        let prog = parse_and_lower_module("export const x = 1;");
        assert!(matches!(prog, Program::Script(_)));
    }

    // ===== Binary Operators =====

    #[test]
    fn test_lower_binary_add() {
        let stmt = first_stmt("1 + 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_subtract() {
        let stmt = first_stmt("3 - 4");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_multiply() {
        let stmt = first_stmt("5 * 6");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_divide() {
        let stmt = first_stmt("10 / 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_modulo() {
        let stmt = first_stmt("10 % 3");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_power() {
        let stmt = first_stmt("2 ** 3");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_bitwise_and() {
        let stmt = first_stmt("5 & 3");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_bitwise_or() {
        let stmt = first_stmt("5 | 3");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_bitwise_xor() {
        let stmt = first_stmt("5 ^ 3");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_left_shift() {
        let stmt = first_stmt("1 << 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_right_shift() {
        let stmt = first_stmt("4 >> 1");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_unsigned_right_shift() {
        let stmt = first_stmt("-4 >>> 1");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_compare_eq() {
        let stmt = first_stmt("1 == 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_compare_strict_eq() {
        let stmt = first_stmt("1 === 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_compare_ne() {
        let stmt = first_stmt("1 != 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_compare_strict_ne() {
        let stmt = first_stmt("1 !== 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_compare_lt() {
        let stmt = first_stmt("1 < 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_compare_le() {
        let stmt = first_stmt("1 <= 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_compare_gt() {
        let stmt = first_stmt("2 > 1");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_compare_ge() {
        let stmt = first_stmt("2 >= 1");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_instanceof() {
        let stmt = first_stmt("obj instanceof Array");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_binary_in() {
        let stmt = first_stmt("'key' in obj");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Unary Operators =====

    #[test]
    fn test_lower_unary_not() {
        let stmt = first_stmt("!true");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_unary_negate() {
        let stmt = first_stmt("-42");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_unary_plus() {
        let stmt = first_stmt("+'-5'");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_unary_bitwise_not() {
        let stmt = first_stmt("~0");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_unary_typeof() {
        let stmt = first_stmt("typeof x");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_unary_void() {
        let stmt = first_stmt("void 0");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_unary_delete() {
        let stmt = first_stmt("delete obj.prop");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Logical Operators =====

    #[test]
    fn test_lower_logical_and() {
        let stmt = first_stmt("a && b");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_logical_or() {
        let stmt = first_stmt("a || b");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_logical_nullish() {
        let stmt = first_stmt("a ?? b");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Assignment =====

    #[test]
    fn test_lower_assign_simple() {
        let stmt = first_stmt("x = 5");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_add() {
        let stmt = first_stmt("x += 5");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_subtract() {
        let stmt = first_stmt("x -= 5");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_multiply() {
        let stmt = first_stmt("x *= 5");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_divide() {
        let stmt = first_stmt("x /= 5");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_modulo() {
        let stmt = first_stmt("x %= 5");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_power() {
        let stmt = first_stmt("x **= 2;");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_bitwise_and() {
        let stmt = first_stmt("x &= 5");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_bitwise_or() {
        let stmt = first_stmt("x |= 5");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_bitwise_xor() {
        let stmt = first_stmt("x ^= 5");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_left_shift() {
        let stmt = first_stmt("x <<= 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_right_shift() {
        let stmt = first_stmt("x >>= 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_assign_unsigned_right_shift() {
        let stmt = first_stmt("x >>>= 2");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Statements =====

    #[test]
    fn test_lower_empty_stmt() {
        let stmt = first_stmt(";");
        assert!(matches!(stmt, Statement::Empty));
    }

    #[test]
    fn test_lower_expr_stmt() {
        let stmt = first_stmt("42;");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_block_stmt() {
        let stmt = first_stmt("{ }");
        assert!(matches!(stmt, Statement::Block(_)));
    }

    #[test]
    fn test_lower_var_decl() {
        let stmt = first_stmt("var x = 5;");
        assert!(matches!(stmt, Statement::VarDeclaration { .. }));
    }

    #[test]
    fn test_lower_let_decl() {
        let stmt = first_stmt("let y = 10;");
        assert!(matches!(stmt, Statement::VarDeclaration { .. }));
    }

    #[test]
    fn test_lower_const_decl() {
        let stmt = first_stmt("const z = 15;");
        assert!(matches!(stmt, Statement::VarDeclaration { .. }));
    }

    // ===== Control Flow =====

    #[test]
    fn test_lower_if_stmt() {
        let stmt = first_stmt("if (true) {}");
        assert!(matches!(stmt, Statement::If { .. }));
    }

    #[test]
    fn test_lower_if_else_stmt() {
        let stmt = first_stmt("if (true) {} else {}");
        assert!(matches!(stmt, Statement::If { .. }));
    }

    #[test]
    fn test_lower_while_stmt() {
        let stmt = first_stmt("while (false) {}");
        assert!(matches!(stmt, Statement::While { .. }));
    }

    #[test]
    fn test_lower_do_while_stmt() {
        let stmt = first_stmt("do {} while (false)");
        assert!(matches!(stmt, Statement::DoWhile { .. }));
    }

    #[test]
    fn test_lower_for_stmt() {
        let stmt = first_stmt("for (;;) {}");
        assert!(matches!(stmt, Statement::For { .. }));
    }

    #[test]
    fn test_lower_for_in_stmt() {
        let stmt = first_stmt("for (x in obj) {}");
        // ForIn is wrapped in Expression variant
        let is_for_in = matches!(&stmt, Statement::Expression(e) if matches!(e.as_ref(), crate::ast::Expression::ForIn { .. }));
        assert!(is_for_in);
    }

    #[test]
    fn test_lower_for_of_stmt() {
        let stmt = first_stmt("for (x of arr) {}");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_for_of_member_lhs() {
        let stmt = first_stmt("for (obj.x of arr) {}");
        let for_of = match &stmt {
            Statement::Expression(e) => match e.as_ref() {
                Expression::ForOf { variable, .. } => variable.as_ref(),
                other => panic!("expected ForOf, got {:?}", other),
            },
            other => panic!("expected Expression(ForOf), got {:?}", other),
        };
        assert!(matches!(for_of, Expression::Member { .. }));
    }

    #[test]
    fn test_lower_switch_stmt() {
        let stmt = first_stmt("switch (x) { case 1: break; }");
        // Switch is lowered to a For loop
        assert!(matches!(stmt, Statement::For { .. }));
    }

    #[test]
    fn test_lower_try_stmt() {
        let stmt = first_stmt("try {} catch (e) {}");
        assert!(matches!(stmt, Statement::Try { .. }));
    }

    #[test]
    fn test_lower_try_finally_stmt() {
        let stmt = first_stmt("try {} catch (e) {} finally {}");
        assert!(matches!(stmt, Statement::Try { .. }));
    }

    #[test]
    fn test_lower_throw_stmt() {
        let stmt = first_stmt("throw new Error();");
        assert!(matches!(stmt, Statement::Throw(_)));
    }

    #[test]
    fn test_lower_return_stmt() {
        let stmt = first_stmt("return 42;");
        assert!(matches!(stmt, Statement::Return(_)));
    }

    // ===== Functions =====

    #[test]
    fn test_lower_function_decl() {
        let stmt = first_stmt("function foo() {}");
        assert!(matches!(stmt, Statement::FunctionDeclaration { .. }));
    }

    #[test]
    fn test_lower_arrow_expr() {
        let stmt = first_stmt("() => 1");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_arrow_with_param() {
        let stmt = first_stmt("x => x + 1");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_arrow_with_params() {
        let stmt = first_stmt("(a, b) => a + b");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_arrow_with_block_body() {
        let stmt = first_stmt("() => { return 1; }");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Class =====

    #[test]
    fn test_lower_class_decl() {
        let stmt = first_stmt("class Foo {}");
        assert!(matches!(stmt, Statement::ClassDeclaration { .. }));
    }

    #[test]
    fn test_lower_class_with_method() {
        let stmt = first_stmt("class Foo { method() {} }");
        assert!(matches!(stmt, Statement::ClassDeclaration { .. }));
    }

    #[test]
    fn test_lower_class_with_field() {
        let stmt = first_stmt("class Foo { x = 1; }");
        assert!(matches!(stmt, Statement::ClassDeclaration { .. }));
    }

    // ===== Literals =====

    #[test]
    fn test_lower_number_literal() {
        let stmt = first_stmt("42");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_string_literal() {
        let stmt = first_stmt("'hello'");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_boolean_literal() {
        let stmt = first_stmt("true");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_null_literal() {
        let stmt = first_stmt("null");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_undefined_literal() {
        let stmt = first_stmt("undefined");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Objects =====

    #[test]
    fn test_lower_object_expr() {
        let stmt = first_stmt("({})");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_object_with_prop() {
        let stmt = first_stmt("({ a: 1 })");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_object_shorthand() {
        let stmt = first_stmt("({ a })");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_object_method() {
        let stmt = first_stmt("({ method() {} })");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_object_getter() {
        let stmt = first_stmt("({ get x() {} })");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_object_setter() {
        let stmt = first_stmt("({ set x(v) {} })");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Arrays =====

    #[test]
    fn test_lower_array_expr() {
        let stmt = first_stmt("[]");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_array_with_elements() {
        let stmt = first_stmt("[1, 2, 3]");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_array_with_spread() {
        let stmt = first_stmt("[...arr]");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Member Access =====

    #[test]
    fn test_lower_member_access() {
        let stmt = first_stmt("obj.prop");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_computed_member() {
        let stmt = first_stmt("obj[0]");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_call_expr() {
        let stmt = first_stmt("foo()");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_new_expr() {
        let stmt = first_stmt("new Foo()");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Template Literals =====

    #[test]
    fn test_lower_template_literal() {
        let stmt = first_stmt("`hello`");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_template_with_expr() {
        let stmt = first_stmt("`hello ${name}`");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Optional Chaining =====

    #[test]
    fn test_lower_optional_chain() {
        let stmt = first_stmt("obj?.prop");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_optional_call() {
        let stmt = first_stmt("obj?.method()");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_optional_index() {
        let stmt = first_stmt("arr?.[0]");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== RegExp =====

    #[test]
    fn test_lower_regexp() {
        let stmt = first_stmt("/pattern/g");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Import/Export =====

    #[test]
    fn test_lower_import_decl() {
        let prog = parse_and_lower_module("import foo from 'mod';");
        assert!(matches!(prog, Program::Script(_)));
    }

    #[test]
    fn test_lower_export_decl() {
        let prog = parse_and_lower_module("export const x = 1;");
        assert!(matches!(prog, Program::Script(_)));
    }

    // ===== Conditional =====

    #[test]
    fn test_lower_conditional() {
        let stmt = first_stmt("a ? b : c");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Sequence =====

    #[test]
    fn test_lower_sequence() {
        let stmt = first_stmt("(a, b, c)");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Await =====

    #[test]
    fn test_lower_await() {
        let stmt = first_stmt("await promise");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Yield =====

    #[test]
    fn test_lower_yield() {
        let stmt = first_stmt("yield value");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== Break/Continue =====

    #[test]
    fn test_lower_break() {
        let stmt = first_stmt("break;");
        assert!(matches!(stmt, Statement::Break(_)));
    }

    #[test]
    fn test_lower_continue() {
        let stmt = first_stmt("continue;");
        assert!(matches!(stmt, Statement::Continue(_)));
    }

    #[test]
    fn test_lower_break_with_label() {
        let stmt = first_stmt("break label;");
        assert!(matches!(stmt, Statement::Break(_)));
    }

    #[test]
    fn test_lower_continue_with_label() {
        let stmt = first_stmt("continue label;");
        assert!(matches!(stmt, Statement::Continue(_)));
    }

    // ===== Labels =====

    #[test]
    fn test_lower_labeled_stmt() {
        let stmt = first_stmt("label: x = 1;");
        assert!(matches!(stmt, Statement::Labeled { .. }));
    }

    // ===== With =====

    #[test]
    fn test_lower_with_stmt() {
        let stmt = first_stmt("with (obj) {}");
        assert!(matches!(stmt, Statement::With { .. }));
    }

    // ===== Debugger (lowered to Empty) =====

    #[test]
    fn test_lower_debugger() {
        let stmt = first_stmt("debugger;");
        assert!(matches!(stmt, Statement::Empty));
    }

    // ===== Nested structures =====

    #[test]
    fn test_lower_nested_expr() {
        let stmt = first_stmt("((a + b) * c) - d");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_complex_arrow() {
        let stmt = first_stmt("async (a, b = 1) => { return a + b; }");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    // ===== JSX =====

    #[test]
    fn test_lower_jsx_element() {
        let stmt = first_stmt("<div />");
        assert!(matches!(stmt, Statement::Expression(_)));
    }

    #[test]
    fn test_lower_jsx_fragment() {
        let stmt = first_stmt("<>text</>");
        assert!(matches!(stmt, Statement::Expression(_)));
    }
}
