//! Integration tests for runts
//!
//! These tests verify the end-to-end transpilation pipeline.

#[cfg(test)]
mod tests {
    use runts_lib::transpile::{Parser, Analyzer, CodeGenerator};
    use std::path::PathBuf;
    
    #[test]
    fn test_parse_simple_component() {
        let source = r#"
interface CounterProps {
    initial?: number;
}

export default function Counter({ initial = 0 }: CounterProps) {
    const [count, setCount] = useState(initial);
    return <div>{count}</div>;
}
"#;
        
        let mut parser = Parser::new();
        let module = parser.parse_source(source).unwrap();
        
        assert!(!module.items.is_empty());
    }
    
    #[test]
    fn test_parse_with_hooks() {
        let source = r#"
import { useState, useEffect } from "preact/hooks";

interface Props {
    initial: number;
}

export default function Counter({ initial }: Props) {
    const [count, setCount] = useState(initial);
    
    useEffect(() => {
        console.log("Mounted");
        return () => console.log("Unmounted");
    }, []);
    
    return (
        <div class="counter">
            <p>{count}</p>
            <button onClick={() => setCount(count + 1)}>+</button>
        </div>
    );
}
"#;
        
        let mut parser = Parser::new();
        let module = parser.parse_source(source).unwrap();
        
        // Verify module structure
        assert!(!module.items.is_empty());
    }
    
    #[test]
    fn test_parse_route_handler() {
        let source = r#"
import { PageProps } from "$fresh/server.ts";

interface Data {
    title: string;
    content: string;
}

interface Props extends PageProps {
    data: Data;
}

export default function BlogPost({ params, data }: Props) {
    return (
        <article>
            <h1>{data.title}</h1>
            <div>{data.content}</div>
        </article>
    );
}

export const handler = {
    async GET(req: Request, ctx: HandlerContext) {
        const post = await getPost(ctx.params.slug);
        return ctx.render({ 
            title: post.title, 
            content: post.content 
        });
    }
};
"#;
        
        let mut parser = Parser::new();
        let module = parser.parse_source(source).unwrap();
        
        assert!(!module.items.is_empty());
    }
    
    #[test]
    fn test_parse_middleware() {
        let source = r#"
import { FreshContext } from "$fresh/server.ts";

export default async function handler(
    req: Request,
    ctx: FreshContext,
) {
    // Add request ID
    ctx.state.requestId = crypto.randomUUID();
    
    // Continue to handler
    return await ctx.next();
}
"#;
        
        let mut parser = Parser::new();
        let module = parser.parse_source(source).unwrap();
        
        assert!(!module.items.is_empty());
    }
    
    #[test]
    fn test_parse_layout() {
        let source = r#"
interface LayoutProps {
    Component: any;
    props: any;
}

export default function Layout({ Component, props }: LayoutProps) {
    return (
        <div class="layout">
            <header>My Site</header>
            <main>
                <Component {...props} />
            </main>
            <footer>Footer</footer>
        </div>
    );
}
"#;
        
        let mut parser = Parser::new();
        let module = parser.parse_source(source).unwrap();
        
        assert!(!module.items.is_empty());
    }
    
    #[test]
    fn test_codegen_simple_component() {
        let source = r#"
interface CounterProps {
    initial?: number;
}

export default function Counter({ initial = 0 }: CounterProps) {
    const [count, setCount] = useState(initial);
    return <div>{count}</div>;
}
"#;
        
        let mut parser = Parser::new();
        let module = parser.parse_source(source).unwrap();
        
        let mut codegen = CodeGenerator::new();
        let rust_code = codegen.generate_module(&module).unwrap();
        
        // Verify generated code contains expected elements
        assert!(rust_code.contains("pub fn counter"));
        assert!(rust_code.contains("use_state"));
        assert!(rust_code.contains("VNode"));
    }
    
    #[test]
    fn test_codegen_with_signals() {
        let source = r#"
import { signal } from "@preact/signals";

export default function Counter() {
    const count = signal(0);
    
    return (
        <div class="counter">
            <p>{count.value}</p>
            <button onClick={() => count.value++}>+</button>
        </div>
    );
}
"#;
        
        let mut parser = Parser::new();
        let module = parser.parse_source(source).unwrap();
        
        let mut codegen = CodeGenerator::new();
        let rust_code = codegen.generate_module(&module).unwrap();
        
        // Verify signals are handled
        assert!(rust_code.contains("pub fn counter"));
    }
    
    #[test]
    fn test_jsx_transform() {
        let source = r#"
export default function Button() {
    return (
        <button class="btn" onClick={() => alert("hi")}>
            Click me
        </button>
    );
}
"#;
        
        let mut parser = Parser::new();
        let module = parser.parse_source(source).unwrap();
        
        let mut codegen = CodeGenerator::new();
        let rust_code = codegen.generate_module(&module).unwrap();
        
        // Verify JSX is transformed
        assert!(rust_code.contains("class_name"));
        assert!(rust_code.contains("on_click"));
    }
    
    #[test]
    fn test_fragment_transform() {
        let source = r#"
export default function FragmentExample() {
    return (
        <>
            <h1>Title</h1>
            <p>Paragraph</p>
        </>
    );
}
"#;
        
        let mut parser = Parser::new();
        let module = parser.parse_source(source).unwrap();
        
        let mut codegen = CodeGenerator::new();
        let rust_code = codegen.generate_module(&module).unwrap();
        
        // Verify Fragment is handled
        assert!(rust_code.contains("Fragment"));
    }
}
