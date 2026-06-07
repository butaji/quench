//! End-to-end round-trip integration tests
//!
//! Tests the full pipeline: TS source → Parser → HIR → Codegen → Rust output
//!

#[cfg(test)]
mod spec_roundtrip_tests {
    use proc_macro2::TokenStream;
    use quote::{ToTokens, quote};

    use crate::transpile::hir::{
        Decl, Expr, FunctionDecl, Module, ModuleItem, QuoteCodegen, Stmt, Type,
    };
    use crate::transpile::parser::TsParser;

    /// Helper: run full pipeline and return (HIR module, generated Rust string)
    fn run_pipeline(source: &str) -> (crate::transpile::hir::Module, String) {
        let parser = TsParser::new();
        let module = parser.parse_tsx(source).expect("parse should succeed");
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
                ModuleItem::Decl(Decl::Variable(var)) => {
                    if let Some(var_tokens) = cg.gen_stmt(&Stmt::Variable(var.clone())) {
                        all_tokens.extend(var_tokens);
                    }
                }
                ModuleItem::Decl(Decl::Type(type_decl)) => {
                    let type_name = syn::Ident::new(&type_decl.name, proc_macro2::Span::call_site());
                    let type_tokens = cg.gen_type(&type_decl.type_);
                    let token = quote! { pub type #type_name = #type_tokens; };
                    all_tokens.extend(token);
                }
                _ => {}
            }
        }

        (module, all_tokens.to_string())
    }

    /// Helper: count Value::Null occurrences (excluding legitimate uses for null/undefined)
    fn count_value_null(s: &str) -> usize {
        s.matches("Value::Null").count()
    }

    /// Helper: check that generated Rust contains expected patterns
    fn assert_rust_contains(rust: &str, patterns: &[&str]) {
        for pattern in patterns {
            assert!(
                rust.contains(pattern),
                "Generated Rust should contain '{}', but got:\n{}",
                pattern,
                rust
            );
        }
    }

    // =============================================================================
    // Category 1: Hello World Function
    // =============================================================================

    #[test]
    fn test_roundtrip_hello_world_function() {
        let source = r#"
function greet(name: string): string {
    return "Hello, " + name + "!";
}
"#;
        let (module, rust) = run_pipeline(source);

        // 1. Parse succeeds - module has items
        assert!(!module.items.is_empty(), "Module should have items");

        // 2. HIR contains function declaration
        let has_function = module.items.iter().any(|item| {
            matches!(item, ModuleItem::Decl(Decl::Function(_)))
        });
        assert!(has_function, "Module should have function declaration");

        // 3. Codegen produces non-empty output
        assert!(!rust.is_empty(), "Generated Rust should not be empty");

        // 4. Generated Rust contains expected patterns
        assert_rust_contains(&rust, &["fn greet"]);

        // 5. No excessive Value::Null fallbacks
        let null_count = count_value_null(&rust);
        assert!(
            null_count <= 2,
            "Generated code has {} Value::Null (expected <= 2), output:\n{}",
            null_count,
            rust
        );
    }

    // =============================================================================
    // Category 2: Counter Component (non-interactive)
    // =============================================================================

    #[test]
    #[ignore = "interface/type parsing not fully implemented for roundtrip tests"]
    fn test_roundtrip_counter_component() {
        let source = r#"
interface CounterProps {
    initial?: number;
}

export default function Counter({ initial = 0 }: CounterProps) {
    return (
        <div class="counter">
            <p>Count: {initial}</p>
        </div>
    );
}
"#;
        let (module, rust) = run_pipeline(source);
        assert!(!module.items.is_empty(), "Module should have items");
        check_module_has_type(&module);
        check_module_has_function(&module, "Counter");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn Counter"]);
    }

    fn check_module_has_type(module: &Module) {
        let has_type = module.items.iter().any(|item| {
            matches!(item, ModuleItem::Decl(Decl::Type(_)))
        });
        assert!(has_type, "Module should have type declaration");
    }

    fn check_module_has_function(module: &Module, name: &str) {
        let has_fn = module.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                f.name == name
            } else {
                false
            }
        });
        assert!(has_fn, "Module should have {} function", name);
    }

    // =============================================================================
    // Category 3: Route Handler + Page
    // =============================================================================

    #[test]
    #[ignore = "interface/type parsing not fully implemented for roundtrip tests"]
    fn test_roundtrip_route_handler_page() {
        let source = r#"
interface HomeData {
    greeting: string;
}

export default function Home({ data }: HomeData) {
    return <h1>{data.greeting}</h1>;
}
"#;
        let (module, rust) = run_pipeline(source);

        // 1. Parse succeeds
        assert!(!module.items.is_empty(), "Module should have items");

        // 2. HIR contains interface
        let has_interface = module.items.iter().any(|item| {
            matches!(item, ModuleItem::Decl(Decl::Type(_)))
        });
        assert!(has_interface, "Module should have interface/type");

        // 3. Codegen produces non-empty output
        assert!(!rust.is_empty(), "Generated Rust should not be empty");

        // 4. Generated Rust contains function
        assert_rust_contains(&rust, &["fn Home"]);
    }

    // =============================================================================
    // Category 4: Type-Directed Lowering (String Union -> Enum)
    // =============================================================================

    #[test]
    fn test_roundtrip_type_lowering_string_union() {
        let source = r#"
type Status = "ok" | "err" | "pending";

function handle(status: Status): string {
    switch (status) {
        case "ok": return "All good";
        case "err": return "Error!";
        case "pending": return "Loading...";
    }
}
"#;
        let (module, rust) = run_pipeline(source);

        // 1. Parse succeeds
        assert!(!module.items.is_empty(), "Module should have items");

        // 2. HIR contains function with switch statement
        let has_switch = module.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                if let Some(ref body) = f.body {
                    body.0.iter().any(|s| matches!(s, Stmt::Switch { .. }))
                } else {
                    false
                }
            } else {
                false
            }
        });
        assert!(has_switch, "Module should have function with switch");

        // 3. Codegen produces non-empty output
        assert!(!rust.is_empty(), "Generated Rust should not be empty");

        // 4. Generated Rust contains match (switch becomes match in Rust)
        assert_rust_contains(&rust, &["match", "fn handle"]);
    }

    // =============================================================================
    // Category 5: Islands Architecture
    // =============================================================================

    #[test]
    #[ignore = "interface/type parsing not fully implemented for roundtrip tests"]
    fn test_roundtrip_islands_component() {
        let source = r#"
interface Props { start: number; }

export default function Counter({ start }: Props) {
    const [count, setCount] = useState(start);
    return (
        <div>
            <p>{count}</p>
            <button onClick={() => setCount(count + 1)}>+</button>
        </div>
    );
}
"#;
        let (module, rust) = run_pipeline(source);
        assert!(!module.items.is_empty(), "Module should have items");
        check_module_has_type_named(&module, "Props");
        check_module_has_function(&module, "Counter");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn Counter"]);
    }

    fn check_module_has_type_named(module: &Module, name: &str) {
        let has_type = module.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Type(t)) = item {
                t.name == name
            } else {
                false
            }
        });
        assert!(has_type, "Module should have {} type", name);
    }

    // =============================================================================
    // Category 6: Complex Mixed Example
    // =============================================================================

    #[test]
    #[ignore = "interface/type parsing not fully implemented for roundtrip tests"]
    fn test_roundtrip_complex_mixed_example() {
        let source = r#"
interface Post { id: number; title: string; }

export const handler = {
    GET: async (_req: Request, ctx: HandlerContext) => {
        const posts: Post[] = await fetchPosts();
        return ctx.render({ posts });
    }
};

export default function Blog({ data }: PageProps<{ posts: Post[] }>) {
    return (
        <div>
            <h1>Blog</h1>
            {data.posts.map(post => (
                <article key={post.id}>
                    <h2>{post.title}</h2>
                </article>
            ))}
        </div>
    );
}
"#;
        let (module, rust) = run_pipeline(source);
        assert!(!module.items.is_empty(), "Module should have items");
        check_module_has_type_named(&module, "Post");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn Blog"]);
    }

    // =============================================================================
    // Additional Round-trip Tests
    // =============================================================================

    #[test]
    fn test_roundtrip_async_function() {
        let source = r#"
async function fetchData(url: string): Promise<Response> {
    const response = await fetch(url);
    return response;
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        let has_async_fn = module.items.iter().any(|item| {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                f.is_async
            } else {
                false
            }
        });
        assert!(has_async_fn, "Module should have async function");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["async", "fn fetchData"]);
    }

    #[test]
    fn test_roundtrip_binary_expressions() {
        let source = r#"
function calc(a: number, b: number): number {
    const add = a + b;
    const sub = a - b;
    const mul = a * b;
    const div = a / b;
    return add + sub * mul - div;
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn calc", "+", "-", "*", "/"]);
    }

    #[test]
    fn test_roundtrip_logical_expressions() {
        let source = r#"
function check(a: boolean, b: boolean): boolean {
    return a && b || false;
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn check", "&&", "||"]);
    }

    #[test]
    fn test_roundtrip_conditional() {
        let source = r#"
function test(x: number): string {
    if (x > 0) {
        return "positive";
    } else if (x < 0) {
        return "negative";
    } else {
        return "zero";
    }
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn test", "if", ">"]);
    }

    #[test]
    fn test_roundtrip_array_literal() {
        let source = r#"
function getNumbers(): number[] {
    return [1, 2, 3, 4, 5];
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn getNumbers", "vec"]);
    }

    #[test]
    fn test_roundtrip_object_literal() {
        let source = r#"
function getConfig(): { name: string; value: number } {
    return { name: "test", value: 42 };
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn getConfig"]);
    }

    #[test]
    fn test_roundtrip_for_loop() {
        let source = r#"
function sum(arr: number[]): number {
    let total = 0;
    for (let i = 0; i < arr.length; i++) {
        total += arr[i];
    }
    return total;
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn sum", "for"]);
    }

    #[test]
    fn test_roundtrip_while_loop() {
        let source = r#"
function countdown(n: number): void {
    while (n > 0) {
        n = n - 1;
    }
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn countdown", "while"]);
    }

    #[test]
    fn test_roundtrip_try_catch() {
        let source = r#"
function safe(): number {
    try {
        return risky();
    } catch (e) {
        return -1;
    }
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn safe", "catch_unwind"]);
    }

    #[test]
    fn test_roundtrip_arrow_function() {
        let source = r#"
const add = (a: number, b: number): number => a + b;
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["|"]);
    }

    #[test]
    fn test_roundtrip_template_literal() {
        let source = r#"
function greet(name: string): string {
    return `Hello, ${name}!`;
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn greet"]);
    }

    // =============================================================================
    // Ignored tests for features not yet fully implemented
    // =============================================================================

    #[test]
    #[ignore]
    fn test_roundtrip_jsx_full() {
        // JSX codegen not yet fully implemented
        let source = r#"
export default function Page() {
    return <div class="test">Hello</div>;
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        // JSX generates Value::Null currently, so we just verify it runs
    }

    #[test]
    #[ignore]
    fn test_roundtrip_use_state_hook() {
        // useState hook handling not yet implemented
        let source = r#"
import { useState } from "preact/hooks";

export default function Counter() {
    const [count, setCount] = useState(0);
    return <div>{count}</div>;
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        // Currently produces Value::Null for useState
    }

    #[test]
    #[ignore]
    fn test_roundtrip_on_click_handler() {
        // onClick handler parsing not yet fully codegen'd
        let source = r#"
export default function Button() {
    return <button onClick={() => alert("clicked")}>Click me</button>;
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        // onClick generates Value::Null currently
    }

    // =============================================================================
    // Stress tests with larger realistic code
    // =============================================================================

    const DATA_PIPELINE_SOURCE: &str = r#"
interface User {
    id: number;
    name: string;
    email: string;
}

interface Order {
    id: string;
    userId: number;
    total: number;
    status: "pending" | "completed" | "cancelled";
}

export async function processOrders(users: User[], orders: Order[]): Promise<Map<number, Order[]>> {
    const result = new Map();

    for (const order of orders) {
        const userOrders = result.get(order.userId) || [];
        userOrders.push(order);
        result.set(order.userId, userOrders);
    }

    const completed: Order[] = [];
    const pending: Order[] = [];

    for (const order of orders) {
        if (order.status === "completed") {
            completed.push(order);
        } else if (order.status === "pending") {
            pending.push(order);
        }
    }

    return result;
}

export function getUserStats(orders: Order[]): { total: number; avg: number } {
    const total = orders.reduce((sum, o) => sum + o.total, 0);
    const avg = orders.length > 0 ? total / orders.length : 0;
    return { total, avg };
}
"#;

    #[test]
    #[ignore = "interface/type parsing not fully implemented for roundtrip tests"]
    fn test_roundtrip_data_pipeline() {
        let (module, rust) = run_pipeline(DATA_PIPELINE_SOURCE);
        assert!(!module.items.is_empty(), "Module should have items");
        check_pipeline_interfaces(&module);
        check_pipeline_functions(&module);
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn processOrders", "fn getUserStats"]);
        check_null_fallback_count(&rust, 5);
    }

    fn check_pipeline_interfaces(module: &Module) {
        check_module_has_type_named(module, "User");
        check_module_has_type_named(module, "Order");
    }

    fn check_pipeline_functions(module: &Module) {
        check_module_has_function(module, "processOrders");
        check_module_has_function(module, "getUserStats");
    }

    fn check_null_fallback_count(rust: &str, max_allowed: usize) {
        let null_count = count_value_null(rust);
        assert!(
            null_count <= max_allowed,
            "Generated code has {} Value::Null (expected <= {}), output:\n{}",
            null_count,
            max_allowed,
            rust
        );
    }

    #[test]
    fn test_roundtrip_form_validation() {
        let source = r#"
interface FormData {
    name: string;
    email: string;
    age?: number;
}

interface ValidationResult {
    valid: boolean;
    errors: string[];
}

export function validateForm(data: FormData): ValidationResult {
    const errors: string[] = [];

    if (!data.name || data.name.length < 2) {
        errors.push("Name must be at least 2 characters");
    }

    if (!data.email || !data.email.includes("@")) {
        errors.push("Invalid email address");
    }

    if (data.age !== undefined && data.age < 0) {
        errors.push("Age cannot be negative");
    }

    return {
        valid: errors.length === 0,
        errors
    };
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn validateForm"]);
    }

    #[test]
    fn test_roundtrip_recursive_tree() {
        let source = r#"
interface TreeNode {
    value: number;
    left?: TreeNode;
    right?: TreeNode;
}

export function traverse(node: TreeNode): number[] {
    const result: number[] = [];

    function inorder(n: TreeNode): void {
        if (n.left) {
            inorder(n.left);
        }
        result.push(n.value);
        if (n.right) {
            inorder(n.right);
        }
    }

    inorder(node);
    return result;
}
"#;
        let (module, rust) = run_pipeline(source);

        assert!(!module.items.is_empty(), "Module should have items");
        assert!(!rust.is_empty(), "Generated Rust should not be empty");
        assert_rust_contains(&rust, &["fn traverse", "fn inorder"]);
    }
}