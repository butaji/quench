#[cfg(test)]
mod parser_tests {
    use crate::transpile::hir::*;
    use crate::transpile::parser::TsParser;

    #[test]
    fn test_parse_import() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"import { useState } from "preact/hooks";"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_type_alias() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"type Props = { count: number; };"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_interface() {
        let parser = TsParser::new();
        let result = parser.parse_source(
            r#"interface CounterProps { initial?: number; step?: number; label?: string; }"#,
        );
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_function() {
        let parser = TsParser::new();
        let result =
            parser.parse_source(r#"function add(a: number, b: number): number { return a + b; }"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_async_function() {
        let parser = TsParser::new();
        let result = parser.parse_source(
            r#"async function fetchData(url: string): Promise<Response> { return fetch(url); }"#,
        );
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_jsx_element() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const elem = <div>Hello</div>;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_jsx_fragment() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const elem = <>Hello <span>world</span></>;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_jsx_fragment_empty() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const elem = <></>;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_jsx_component() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const comp = <Counter initial={0} step={1} />;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_template_literal() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const msg = `Hello ${name}`;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_destructuring_object() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const { name, age } = person;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_destructuring_array() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const [first, ...rest] = items;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_conditional() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const result = count > 0 ? "positive" : "negative";"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_logical_operators() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const a = x && y || z;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_use_state() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const [count, setCount] = useState(0);"#);
        assert!(result.is_ok());
    }
}
