//! SWC parser integration
//!
//! Uses swc to parse JavaScript/JSX source code into the swc AST,
//! then lower to our runtime AST via lower.rs.

use swc_common::{
    sync::Lrc,
    FileName, SourceMap,
};
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, EsSyntax};
use crate::ast::Program;
use crate::value::JsError;
use crate::lower::lower_script;

/// Parse JavaScript source using swc (script mode, not module)
pub fn parse_swc(source: &str) -> Result<Program, JsError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("input".into())),
        source.to_string(),
    );
    
    let lexer = Lexer::new(
        Syntax::Es(EsSyntax {
            jsx: false,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    
    let mut parser = Parser::new_from(lexer);
    
    // Use parse_script for regular JS code (not ES modules)
    let script = parser.parse_script().map_err(|e| {
        JsError(format!("Parse error: {:?}", e))
    })?;
    
    // Lower swc AST to our runtime AST
    lower_script(&script).map_err(|e| JsError(e.to_string()))
}

/// Parse JavaScript/JSX source using swc (script mode)
#[allow(dead_code)]
pub fn parse_jsx(source: &str) -> Result<Program, JsError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("input".into())),
        source.to_string(),
    );
    
    let lexer = Lexer::new(
        Syntax::Es(EsSyntax {
            jsx: true,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    
    let mut parser = Parser::new_from(lexer);
    
    // Use parse_script for regular JS code
    let script = parser.parse_script().map_err(|e| {
        JsError(format!("Parse error: {:?}", e))
    })?;
    
    lower_script(&script).map_err(|e| JsError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let result = parse_swc("42");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_binary() {
        let result = parse_swc("1 + 2;");
        assert!(result.is_ok(), "Failed: {:?}", result);
        if let Ok(prog) = &result {
            eprintln!("Binary AST: {:?}", prog);
        }
    }

    #[test]
    fn test_parse_var() {
        let result = parse_swc("var x = 1 + 2;");
        eprintln!("Var result: {:?}", result);
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_object() {
        let result = parse_swc(r#"
            const x = { a: 1, b: 2 };
        "#);
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_function() {
        let result = parse_swc("function add(a, b) { return a + b; }");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_arrow() {
        let result = parse_swc("const add = (a, b) => a + b;");
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_parse_runtime_js() {
        // Path is relative to the crate root, go up to workspace root
        let source = std::fs::read_to_string("../../src/runtime.js").unwrap();
        let result = parse_swc(&source);
        assert!(result.is_ok(), "Failed to parse runtime.js: {:?}", result);
    }
}

    #[test]
    fn test_debug_script() {
        use swc_common::{sync::Lrc, FileName, SourceMap};
        use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, EsSyntax};
        
        let source = "1 + 2;";
        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            Lrc::new(FileName::Custom("input".into())),
            source.to_string(),
        );
        
        let lexer = Lexer::new(
            Syntax::Es(EsSyntax {
                jsx: false,
                ..Default::default()
            }),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );
        
        let mut parser = Parser::new_from(lexer);
        let script = parser.parse_script().expect("parse should succeed");
        eprintln!("Script body len: {}", script.body.len());
        for (i, stmt) in script.body.iter().enumerate() {
            eprintln!("  Stmt {}: {:?}", i, stmt);
        }
    }

    #[test]
    fn test_debug_function_decl() {
        use swc_common::{sync::Lrc, FileName, SourceMap};
        use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, EsSyntax};
        
        let source = "function foo() { return 1; }";
        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            Lrc::new(FileName::Custom("input".into())),
            source.to_string(),
        );
        
        let lexer = Lexer::new(
            Syntax::Es(EsSyntax {
                jsx: false,
                ..Default::default()
            }),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );
        
        let mut parser = Parser::new_from(lexer);
        let script = parser.parse_script().expect("parse should succeed");
        eprintln!("Script body len: {}", script.body.len());
        for (i, stmt) in script.body.iter().enumerate() {
            eprintln!("  Stmt {}: {:?}", i, stmt);
        }
    }
