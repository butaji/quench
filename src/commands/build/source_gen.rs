//! Source file generation
//!

use anyhow::{Context, Result};
use regex::Regex;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::commands::build::{ComponentEntry, GeneratedFile, IslandEntry, RouteEntry};
use crate::transpile::hir::{self, QuoteCodegen, Stmt};
use crate::transpile::parser::TsParser;

/// Check if this is a single-file script build (no routes/islands/components)
pub fn is_single_file_build(
    ts_files: &[PathBuf],
    routes: &[RouteEntry],
    islands: &[IslandEntry],
    components: &[ComponentEntry],
) -> bool {
    ts_files.len() == 1 && routes.is_empty() && islands.is_empty() && components.is_empty()
}

/// Get the module name from a TS file path
pub fn ts_file_to_module_name(file: &Path) -> String {
    file.file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("app")
        .replace("-", "_")
        .replace(".", "_")
}

/// Scan components directory for component files
pub fn scan_components(project_root: &Path) -> Vec<ComponentEntry> {
    let components_dir = project_root.join("components");
    let mut components = Vec::new();

    if !components_dir.exists() {
        return components;
    }

    for entry in WalkDir::new(&components_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "tsx" || ext == "ts" {
                let name = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Component")
                    .to_string();

                components.push(ComponentEntry {
                    name,
                    file: path.to_path_buf(),
                });
            }
        }
    }

    components
}

/// Result of generating source files - includes both generated files and HIR for single-file builds
pub struct SourceGenResult {
    pub generated_files: Vec<GeneratedFile>,
    /// For single-file builds: the HIR module items (statements and declarations)
    pub single_file_stmts: Option<Vec<Stmt>>,
}

/// Generate source files from TypeScript using QuoteCodegen
pub fn generate_all(files: &[PathBuf]) -> Result<SourceGenResult, anyhow::Error> {
    let mut generated = Vec::new();
    let parser = TsParser::new();
    let codegen = QuoteCodegen::default();

    // For single-file builds, collect all HIR statements
    let mut all_stmts: Vec<Stmt> = Vec::new();

    for file in files {
        let relative = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("mod.rs")
            .replace(".tsx", ".rs")
            .replace(".ts", ".rs");

        // Parse TypeScript to HIR. The parser chooses JSX mode based on the
        // *file extension* — .tsx enables JSX, .ts disables it. A .tsx file
        // fed to parse_source() raises a parse error on the first JSX
        // attribute (oxc treats `<` as start of a generic and chokes on
        // `class=` inside it), which is why every example project generated
        // `// Parse error:` stubs for its .tsx files.
        let source = std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        let is_tsx = file
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("tsx"))
            .unwrap_or(false);
        let parse_result = if is_tsx {
            parser.parse_tsx(&source)
        } else {
            parser.parse_source(&source)
        };
        let module = match parse_result {
            Ok(m) => m,
            Err(e) => {
                // If parsing fails, generate a stub
                generated.push(GeneratedFile {
                    path: PathBuf::from(format!("src/gen/{}", relative)),
                    content: format!("// Parse error: {}", e),
                });
                continue;
            }
        };

        // Extract both Stmt items and Decl items (functions, variables) for codegen
        let stmts: Vec<_> = module
            .items
            .into_iter()
            .filter_map(|item| match item {
                hir::ModuleItem::Stmt(s) => Some(s),
                hir::ModuleItem::Decl(d) => match d {
                    hir::Decl::Function(func) => Some(hir::Stmt::FunctionDecl(func)),
                    hir::Decl::Variable(var) => Some(hir::Stmt::Variable(var)),
                    _ => None, // Skip type and class declarations for now
                },
                _ => None,
            })
            .collect();

        // Collect for single-file builds
        all_stmts.extend(stmts.clone());

        let tokens = codegen.gen_module(&stmts);
        let rust_code = tokens.to_string();

        generated.push(GeneratedFile {
            path: PathBuf::from(format!("src/gen/{}", relative)),
            content: format!("// Generated from {}\n\n{}", file.display(), rust_code),
        });
    }

    Ok(SourceGenResult {
        generated_files: generated,
        single_file_stmts: if all_stmts.is_empty() {
            None
        } else {
            Some(all_stmts)
        },
    })
}

/// Result of generating lib.rs content from HIR
pub struct LibGenResult {
    /// Content for lib.rs (function definitions only)
    pub lib_content: String,
    /// Executable statements for main.rs
    pub exec_stmts: Vec<String>,
    /// Function definitions for main.rs (inline)
    pub fn_defs: Vec<String>,
}

/// Generate lib.rs content directly from HIR statements (for single-file builds)
/// This bypasses TokenStream to_string() which produces malformed code
pub fn generate_lib_from_hir(stmts: &[Stmt], source_path: &Path) -> LibGenResult {
    let codegen = QuoteCodegen::default();
    let (fn_defs, exec_stmts) = collect_stmts(stmts, &codegen);
    let lib_output = build_lib_output(&fn_defs, source_path);
    let exec_stmt_strings = generate_exec_stmts(&exec_stmts, &codegen);
    LibGenResult {
        lib_content: lib_output,
        exec_stmts: exec_stmt_strings,
        fn_defs,
    }
}

fn collect_stmts(stmts: &[Stmt], codegen: &QuoteCodegen) -> (Vec<String>, Vec<Stmt>) {
    let mut fn_defs = Vec::new();
    let mut exec_stmts = Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::FunctionDecl(func) => {
                fn_defs.push(generate_function_string(codegen, func));
            }
            _ => {
                exec_stmts.push(stmt.clone());
            }
        }
    }
    (fn_defs, exec_stmts)
}

fn build_lib_output(fn_defs: &[String], source_path: &Path) -> String {
    let mut lib_output = format!(
        "//! Auto-generated library\n\n// Generated from {}\n\n",
        source_path.display()
    );
    for fn_def in fn_defs {
        lib_output.push_str(fn_def);
        lib_output.push_str("\n\n");
    }
    lib_output
}

fn generate_exec_stmts(exec_stmts: &[Stmt], codegen: &QuoteCodegen) -> Vec<String> {
    exec_stmts
        .iter()
        .filter_map(|stmt| codegen.gen_stmt(stmt).map(|t| t.to_string()))
        .collect()
}

/// Generate a function definition string directly from HIR
/// This avoids TokenStream to_string() issues
fn generate_function_string(
    codegen: &QuoteCodegen,
    func: &crate::transpile::hir::FunctionDecl,
) -> String {
    let fn_name = &func.name;
    let async_kw = if func.is_async { "async " } else { "" };
    let params_str = gen_params_string(func);
    let ret_type_str = gen_ret_type_string(func);
    let body_str = generate_body_string(codegen, &func.body, &ret_type_str);
    let header = make_fn_header(async_kw, fn_name, &params_str, &ret_type_str);
    make_fn_string(&header, &body_str)
}

fn make_fn_header(async_kw: &str, name: &str, params: &str, ret: &str) -> String {
    format!("pub ",)
        .to_string()
        + async_kw
        + "fn "
        + name
        + "("
        + params
        + ") -> "
        + ret
        + " {"
}

fn make_fn_string(header: &str, body: &str) -> String {
    header.to_string() + "\n" + body + "}\n"
}

fn gen_params_string(func: &crate::transpile::hir::FunctionDecl) -> String {
    let params: Vec<String> = func.params.iter().map(gen_param_string).collect();
    params.join(", ")
}

fn gen_param_string(p: &crate::transpile::hir::Param) -> String {
    let name = &p.name;
    let ty_str = p.type_.as_ref()
        .map(|t| type_to_rust_string(t))
        .unwrap_or_else(|| "String".to_string());
    format!("{}: {}", name, ty_str)
}

fn gen_ret_type_string(func: &crate::transpile::hir::FunctionDecl) -> String {
    func.return_type.as_ref()
        .map(|t| type_to_rust_string(t))
        .unwrap_or_else(|| {
            infer_return_type_from_body(&func.body).unwrap_or_else(|| "()".to_string())
        })
}

/// Infer return type from return statements in the body
fn infer_return_type_from_body(body: &Option<crate::transpile::hir::Block>) -> Option<String> {
    use crate::transpile::hir::Stmt;

    let body = match body {
        Some(b) => b,
        None => return None,
    };

    // Look for return statements with expressions
    for stmt in &body.0 {
        if let Stmt::Return { arg: Some(expr) } = stmt {
            // Infer type from expression
            return Some(infer_type_from_expr(expr));
        }
    }
    None
}

/// Infer Rust type from expression
fn infer_type_from_expr(expr: &crate::transpile::hir::Expr) -> String {
    use crate::transpile::hir::Expr as E;
    match expr {
        E::String(_) => "String".to_string(),
        E::Number(_) => "f64".to_string(),
        E::Boolean(_) => "bool".to_string(),
        E::Null | E::Undefined => "Value".to_string(),
        E::Bin {
            op,
            left: _,
            right: _,
        } => {
            // For Add with string operands, result is String
            use crate::transpile::hir::BinaryOp;
            if matches!(op, BinaryOp::Add) {
                "String".to_string()
            } else {
                "Value".to_string()
            }
        }
        E::Call { .. } => "Value".to_string(), // TODO: infer from function return type
        _ => "Value".to_string(),
    }
}

/// Convert HIR Type to Rust string representation
fn type_to_rust_string(ty: &crate::transpile::hir::Type) -> String {
    use crate::transpile::hir::Type as T;
    match ty {
        T::String => "String".to_string(),
        T::Number => "f64".to_string(),
        T::Boolean => "bool".to_string(),
        T::Void | T::Never => "()".to_string(),
        T::Undefined | T::Null | T::Unknown | T::Any => "Value".to_string(),
        T::BigInt => "i64".to_string(),
        T::Array { elem } => format!("Vec<{}>", type_to_rust_string(elem)),
        T::Ref { name, generics } => {
            if generics.is_empty() {
                name.clone()
            } else {
                let inner = generics
                    .iter()
                    .map(type_to_rust_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{}>", name, inner)
            }
        }
        _ => "Value".to_string(),
    }
}

/// Generate function body string from HIR Block
fn generate_body_string(
    codegen: &QuoteCodegen,
    body: &Option<crate::transpile::hir::Block>,
    ret_type: &str,
) -> String {
    use crate::transpile::hir::Stmt;

    let body = match body {
        Some(b) => b,
        None => return "    unimplemented!();\n".to_string(),
    };

    let mut output = String::new();
    let stmts = &body.0;

    for (i, stmt) in stmts.iter().enumerate() {
        if let Some(tokens) = codegen.gen_stmt(stmt) {
            let stmt_str = tokens.to_string();
            let is_last_stmt = i == stmts.len() - 1;

            // Handle return statements: if function returns non-() but body has return,
            // and the return type wasn't declared, we need to handle it
            let stmt_str = if stmt_str.starts_with("return ") && is_last_stmt && ret_type != "()" {
                // Remove "return " prefix and trailing semicolon, just output the expression
                stmt_str["return ".len()..]
                    .trim_end_matches(';')
                    .trim()
                    .to_string()
            } else {
                stmt_str
            };

            // Handle multiline statements
            for line in stmt_str.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // For the last statement, don't add semicolon if it would make it return ()
                // Instead, just output the expression so Rust returns it implicitly
                let line_final =
                    if is_last_stmt && !trimmed.ends_with('{') && !trimmed.ends_with('}') {
                        // Check if this is an expression statement (not control flow)
                        if !trimmed.starts_with("if ")
                            && !trimmed.starts_with("while ")
                            && !trimmed.starts_with("for ")
                            && !trimmed.starts_with("loop ")
                            && !trimmed.starts_with("match ")
                            && !trimmed.starts_with("return ")
                            && !trimmed.ends_with(',')
                            && !trimmed.ends_with(";")
                        {
                            // This is an expression that should be returned
                            trimmed.to_string()
                        } else if trimmed.ends_with(';') {
                            trimmed.trim_end_matches(';').trim().to_string()
                        } else {
                            trimmed.to_string()
                        }
                    } else if trimmed.ends_with('}') || trimmed.ends_with('{') {
                        trimmed.to_string()
                    } else if trimmed.ends_with(';') {
                        trimmed.to_string()
                    } else {
                        format!("{};", trimmed)
                    };
                output.push_str(&format!("    {}\n", line_final));
            }
        }
    }
    output
}

/// Generate lib.rs
/// For single-file builds, the transpiled code is written directly to lib.rs
pub fn generate_lib(
    _routes: &[RouteEntry],
    islands: &[IslandEntry],
    components: &[ComponentEntry],
    source_file: Option<&Path>,
    rust_code: Option<&str>,
) -> String {
    let mut output = String::new();
    output.push_str("//! Auto-generated library\n\n");

    // Check if this is a single-file script build
    let is_script = source_file.is_some() && rust_code.is_some();

    if is_script {
        // Single-file build: lib.rs contains the actual transpiled code wrapped in main()
        let file = source_file.unwrap();
        let code = rust_code.unwrap();

        // For single-file builds, wrap all code in a main() function.
        // We need to remove 'pub' from function declarations since nested functions
        // can't be pub in Rust.
        let wrapped = wrap_in_main_function(code);

        output.push_str(&format!("// Generated from {}\n\n", file.display()));
        output.push_str(&wrapped);
    } else {
        // Multi-file build: standard structure with routes/islands/gen modules
        output.push_str("pub mod routes;\n");
        output.push_str("pub mod islands;\n");
        output.push_str("pub mod gen;\n\n");

        output.push_str(&format!("// {} routes\n", _routes.len()));
        output.push_str(&format!("// {} islands\n", islands.len()));
        output.push_str(&format!("// {} components\n", components.len()));
    }

    output
}

/// Generate main.rs content
/// For single-file builds, the exec_stmts are inlined in main()
pub fn generate_main(
    source_file: Option<&Path>,
    exec_stmts: &[String],
    fn_defs: &[String],
) -> String {
    let mut output = String::new();
    output.push_str("//! Auto-generated main\n\n");

    if source_file.is_some() && !exec_stmts.is_empty() {
        // Single-file build: inline the function definitions and executable statements in main.rs
        // Functions are defined first, then main
        for fn_def in fn_defs {
            output.push_str(fn_def);
            output.push_str("\n\n");
        }
        output.push_str("fn main() {\n");
        for stmt in exec_stmts {
            // Fix string arguments in function calls
            let fixed_stmt = fix_string_arguments(stmt);
            // Handle multiline statements
            for line in fixed_stmt.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Determine if we need to add semicolon
                let line_final = if trimmed.ends_with('}') || trimmed.ends_with('{') {
                    trimmed.to_string()
                } else if trimmed.ends_with(';') {
                    trimmed.to_string()
                } else {
                    format!("{};", trimmed)
                };
                output.push_str(&format!("    {}\n", line_final));
            }
        }
        output.push_str("}\n");
    } else if source_file.is_some() {
        // Single-file but no exec statements - shouldn't happen but handle it
        output.push_str("fn main() {\n");
        output.push_str("    println!(\"No code to run\");\n");
        output.push_str("}\n");
    } else {
        // Multi-file build: default server entry point
        output.push_str("use runts_lib::runtime::prelude::*;\n\n");
        output.push_str("#[tokio::main]\n");
        output.push_str("async fn main() {\n");
        output.push_str("    println!(\"Starting server...\");\n");
        output.push_str("}\n");
    }

    output
}

/// Fix string arguments in function calls - add .to_string() if missing
/// This handles cases where string literals are passed without explicit .to_string()
fn fix_string_arguments(stmt: &str) -> String {
    // Simple regex-like replacement for function calls with string arguments
    // This is a heuristic fix for the codegen issue
    let mut result = stmt.to_string();

    // Find patterns like `func("string")` or `func ("string")` and add .to_string()
    // We look for function calls followed by string arguments
    let re_pattern = regex::Regex::new(r#"(\w+)\s*\(\s*"([^"]+)"\s*\)"#).unwrap();
    result = re_pattern
        .replace_all(&result, |caps: &regex::Captures| {
            let func_name = &caps[1];
            let arg = &caps[2];
            // Don't add .to_string() if already present
            if arg.contains(".to_string()") || arg.contains('(') {
                caps[0].to_string()
            } else {
                format!(r#"{}("{}".to_string())"#, func_name, arg)
            }
        })
        .to_string();

    result
}

/// Generate mod files
pub fn generate_mod_files(build_dir: &Path) -> Result<(), anyhow::Error> {
    use std::fs;

    fs::create_dir_all(build_dir.join("src/gen"))?;
    fs::write(build_dir.join("src/gen/mod.rs"), "//! Generated modules\n")?;

    Ok(())
}

/// Wrap transpiled code in a main() function for single-file builds
fn wrap_in_main_function(code: &str) -> String {
    // Parse items from the generated code
    let (functions, statements) = parse_items(code);
    build_lib_content(&functions, &statements)
}

/// Parse generated code into function definitions and statements
/// Handles the case where TokenStream.to_string() produces code on a single line
fn parse_items(code: &str) -> (Vec<String>, Vec<String>) {
    let mut functions = Vec::new();
    let mut statements = Vec::new();

    // Split by semicolons to handle single-line output from TokenStream
    // But we need to be careful with function bodies that contain semicolons
    let mut current = String::new();
    let mut brace_depth = 0;

    for ch in code.chars() {
        current.push(ch);
        if ch == '{' {
            brace_depth += 1;
        } else if ch == '}' {
            brace_depth -= 1;
            // If we close a brace at depth 0, we might be ending a function definition
            // The next character(s) should be checked
        } else if ch == ';' && brace_depth == 0 {
            // End of a statement
            let item = current.trim().to_string();
            current.clear();

            if item.starts_with("pub fn ") || item.starts_with("fn ") {
                functions.push(item);
            } else if !item.is_empty() {
                statements.push(item);
            }
        }
    }

    // Handle any remaining content
    // Also handle the case where a function definition ends without semicolon
    // (brace_depth should be 0 at the end of a properly closed function)
    if !current.trim().is_empty() {
        let item = current.trim().to_string();
        // Check if this is a function definition that ends with }
        if (item.starts_with("pub fn ") || item.starts_with("fn ")) && item.ends_with('}') {
            functions.push(item);
        } else if !item.is_empty() {
            statements.push(item);
        }
    }

    (functions, statements)
}

/// Build the library content with functions at module level and main() wrapper
fn build_lib_content(functions: &[String], statements: &[String]) -> String {
    let mut output = String::new();
    output.push_str("//! Auto-generated module\n\n");

    for func in functions {
        output.push_str(func);
        output.push_str("\n\n");
    }

    output.push_str("pub fn main() {\n");
    for stmt in statements {
        output.push_str("    ");
        output.push_str(stmt);
        output.push_str("\n");
    }
    output.push_str("}\n");

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Test the file-extension dispatch: a `.tsx` file with JSX should
    /// successfully generate Rust (not a `// Parse error:` stub), and a
    /// `.ts` file with plain TS should also succeed.
    #[test]
    fn generate_all_dispatches_tsx_vs_ts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tsx_path = dir.path().join("Counter.tsx");
        let ts_path = dir.path().join("helper.ts");
        fs::write(
            &tsx_path,
            r#"interface Props { initial?: number }
export default function Counter({ initial = 0 }: Props) {
  return <div class="counter">Count: {initial}</div>;
}
"#,
        )
        .expect("write tsx");
        fs::write(
            &ts_path,
            r#"export const answer: number = 42;
export function double(x: number): number { return x * 2; }
"#,
        )
        .expect("write ts");

        let result = generate_all(&[tsx_path.clone(), ts_path.clone()]).expect("generate_all");
        assert_eq!(result.generated_files.len(), 2);

        for gf in &result.generated_files {
            let body = &gf.content;
            assert!(
                !body.contains("// Parse error"),
                "{} should not be a parse error stub, got: {}",
                gf.path.display(),
                body
            );
        }
    }

    /// Edge case: a `.tsx` file that uses a `class` attribute (the exact
    /// pattern that previously failed when fed through parse_source instead
    /// of parse_tsx).
    #[test]
    fn generate_all_tsx_with_class_attribute_does_not_stub() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tsx_path = dir.path().join("Card.tsx");
        fs::write(
            &tsx_path,
            r#"export default function Card() {
  return <div class="card"><p class="title">hi</p></div>;
}
"#,
        )
        .expect("write");

        let result = generate_all(&[tsx_path]).expect("generate_all");
        assert_eq!(result.generated_files.len(), 1);
        assert!(
            !result.generated_files[0].content.contains("// Parse error"),
            "got: {}",
            result.generated_files[0].content
        );
    }

    /// When a file genuinely fails to parse, generate_all should still
    /// produce a `// Parse error:` stub for that file (so the build is
    /// complete and downstream code can reference it) — but other files
    /// in the batch should be unaffected.
    #[test]
    fn generate_all_parse_error_does_not_block_other_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bad = dir.path().join("bad.ts");
        let good = dir.path().join("good.ts");
        fs::write(&bad, "this is not valid typescript !!! @@@\n").expect("write bad");
        fs::write(&good, "export const x = 1;\n").expect("write good");

        let result = generate_all(&[bad.clone(), good.clone()]).expect("generate_all");
        assert_eq!(result.generated_files.len(), 2);
        let bad_stub = result
            .generated_files
            .iter()
            .find(|g| g.path.to_string_lossy().contains("bad"))
            .expect("bad stub");
        assert!(bad_stub.content.contains("// Parse error"));
        let good_out = result
            .generated_files
            .iter()
            .find(|g| g.path.to_string_lossy().contains("good"))
            .expect("good out");
        assert!(!good_out.content.contains("// Parse error"));
    }
}
