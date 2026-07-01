use quench_runtime::{Context, Program};
use quench_runtime::ast::Statement;

fn count_stmts_and_depth(stmts: &[Statement], depth: usize) -> (usize, usize) {
    let mut total = stmts.len();
    let mut max_depth = depth;
    
    for stmt in stmts {
        match stmt {
            Statement::Block(b) => {
                let (count, d) = count_stmts_and_depth(b, depth + 1);
                total += count;
                max_depth = max_depth.max(d);
            }
            Statement::FunctionDeclaration { body, .. } => {
                let (count, d) = count_stmts_and_depth(body, depth + 1);
                total += count;
                max_depth = max_depth.max(d);
            }
            _ => {}
        }
    }
    
    (total, max_depth)
}

fn main() {
    let source = std::fs::read_to_string("/Users/admin/Code/GitHub/quench/src/runtime.js").unwrap();
    let ctx = Context::new().expect("Failed to create context");
    
    match ctx.parse(&source) {
        Ok(Program::Script(stmts)) => {
            let (count, depth) = count_stmts_and_depth(&stmts, 0);
            println!("Statements: {}, Max nesting depth: {}", count, depth);
        }
        Err(e) => println!("Parse error: {:?}", e),
    }
}

#[test]
fn test_runtime_ink_check() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    
    let runtime = std::fs::read_to_string("/Users/admin/Code/GitHub/quench/src/runtime.js").unwrap();
    ctx.eval(&runtime).expect("runtime load");
    
    // Check ink namespace
    let result = ctx.eval(r#"
        typeof ink
    "#).unwrap();
    eprintln!("typeof ink = {:?}", result);
    
    // Simple render test
    let result = ctx.eval(r#"
        typeof render
    "#).unwrap();
    eprintln!("typeof render = {:?}", result);
    
    // Try a simple component
    let result = ctx.eval(r#"
        function TestComp() {
            return { type: Box, props: { children: "Hello" } };
        }
        try {
            render({ type: TestComp, props: {} });
            "rendered"
        } catch(e) {
            "error: " + String(e)
        }
    "#);
    eprintln!("Simple render test: {:?}", result);
}
