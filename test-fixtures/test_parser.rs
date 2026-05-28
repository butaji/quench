//! Integration tests for the parser with test fixtures

#[cfg(test)]
mod tests {
    use runts_lib::transpile::parser::parse_source;

    /// Test parsing literals
    #[test]
    fn test_string_literals() {
        let source = r#"
            const empty = "";
            const hello = "hello";
            const world = 'world';
            const template = `template`;
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse string literals: {:?}", result.err());
    }

    /// Test parsing numbers
    #[test]
    fn test_number_literals() {
        let source = r#"
            const zero = 0;
            const integer = 42;
            const float = 3.14;
            const hex = 0xFF;
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse number literals: {:?}", result.err());
    }

    /// Test parsing boolean and null
    #[test]
    fn test_boolean_null() {
        let source = r#"
            const t = true;
            const f = false;
            const n = null;
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse boolean/null: {:?}", result.err());
    }

    /// Test parsing function declarations
    #[test]
    fn test_function_declarations() {
        let source = r#"
            function add(a: number, b: number): number {
                return a + b;
            }
            
            function greet(name: string): string {
                return "Hello, " + name;
            }
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse functions: {:?}", result.err());
        
        let module = result.unwrap();
        assert!(!module.items.is_empty(), "Module should have items");
    }

    /// Test parsing binary expressions
    #[test]
    fn test_binary_expressions() {
        let source = r#"
            const add = 1 + 2;
            const sub = 5 - 3;
            const mul = 4 * 3;
            const div = 10 / 2;
            const eq = 5 == 5;
            const strict_eq = 5 === 5;
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse binary expressions: {:?}", result.err());
    }

    /// Test parsing logical expressions
    #[test]
    fn test_logical_expressions() {
        let source = r#"
            const and = true && false;
            const or = true || false;
            const not = !true;
            const coalesce = null ?? "default";
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse logical expressions: {:?}", result.err());
    }

    /// Test parsing conditional expressions
    #[test]
    fn test_conditional() {
        let source = r#"
            const grade = (score: number): string => {
                return score >= 90 ? "A" : "B";
            };
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse conditional: {:?}", result.err());
    }

    /// Test parsing array expressions
    #[test]
    fn test_array_expressions() {
        let source = r#"
            const arr = [1, 2, 3];
            const nested = [[1, 2], [3, 4]];
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse arrays: {:?}", result.err());
    }

    /// Test parsing object expressions
    #[test]
    fn test_object_expressions() {
        let source = r#"
            const obj = { a: 1, b: 2 };
            const nested = { outer: { inner: 1 } };
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse objects: {:?}", result.err());
    }

    /// Test parsing arrow functions
    #[test]
    fn test_arrow_functions() {
        let source = r#"
            const add = (a: number, b: number): number => a + b;
            const greet = (name: string) => "Hello, " + name;
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse arrow functions: {:?}", result.err());
    }

    /// Test parsing template literals
    #[test]
    fn test_template_literals() {
        let source = r#"
            const greeting = `Hello, ${name}!`;
            const multiline = `line1
            line2`;
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse templates: {:?}", result.err());
    }

    /// Test parsing imports
    #[test]
    fn test_imports() {
        let source = r#"
            import { foo } from 'module';
            import bar from 'default';
            import * as ns from 'namespace';
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse imports: {:?}", result.err());
    }

    /// Test parsing exports
    #[test]
    fn test_exports() {
        let source = r#"
            export const a = 1;
            export function foo() { return 1; }
            export default 42;
        "#;
        let result = parse_source(source, false);
        assert!(result.is_ok(), "Failed to parse exports: {:?}", result.err());
    }
}
