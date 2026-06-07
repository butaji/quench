//! Source file generation
//!

use anyhow::{Context, Result};
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
    let mut all_stmts: Vec<Stmt> = Vec::new();

    for file in files {
        if let Some((gf, stmts)) = process_one_file(file, &parser, &codegen)? {
            generated.push(gf);
            all_stmts.extend(stmts);
        }
    }

    Ok(SourceGenResult {
        generated_files: generated,
        single_file_stmts: if all_stmts.is_empty() { None } else { Some(all_stmts) },
    })
}

fn process_one_file(
    file: &PathBuf,
    parser: &TsParser,
    codegen: &QuoteCodegen,
) -> Result<Option<(GeneratedFile, Vec<Stmt>)>, anyhow::Error> {
    let relative = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mod.rs")
        .replace(".tsx", ".rs")
        .replace(".ts", ".rs");

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
            return Ok(Some((
                GeneratedFile {
                    path: PathBuf::from(format!("src/gen/{}", relative)),
                    content: format!("// Parse error: {}", e),
                },
                Vec::new(),
            )));
        }
    };

    let stmts: Vec<_> = module
        .items
        .into_iter()
        .filter_map(|item| match item {
            hir::ModuleItem::Stmt(s) => Some(s),
            hir::ModuleItem::Decl(d) => match d {
                hir::Decl::Function(func) => Some(hir::Stmt::FunctionDecl(func)),
                hir::Decl::Variable(var) => Some(hir::Stmt::Variable(var)),
                _ => None,
            },
            _ => None,
        })
        .collect();

    let tokens = codegen.gen_module(&stmts);
    let rust_code = tokens.to_string();

    Ok(Some((
        GeneratedFile {
            path: PathBuf::from(format!("src/gen/{}", relative)),
            content: format!("// Generated from {}\n\n{}", file.display(), rust_code),
        },
        stmts,
    )))
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

mod fn_gen;
pub(crate) use fn_gen::*;

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
fn parse_items(code: &str) -> (Vec<String>, Vec<String>) {
    let mut functions = Vec::new();
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut brace_depth = 0;

    for ch in code.chars() {
        current.push(ch);
        brace_depth = update_brace_depth(brace_depth, ch);
        if ch == ';' && brace_depth == 0 {
            let item = current.trim().to_string();
            current.clear();
            classify_item(&item, &mut functions, &mut statements);
        }
    }

    if !current.trim().is_empty() {
        let item = current.trim().to_string();
        classify_remaining(&item, &mut functions, &mut statements);
    }

    (functions, statements)
}

fn update_brace_depth(depth: i32, ch: char) -> i32 {
    match ch {
        '{' => depth + 1,
        '}' => depth - 1,
        _ => depth,
    }
}

fn classify_item(item: &str, functions: &mut Vec<String>, statements: &mut Vec<String>) {
    if item.starts_with("pub fn ") || item.starts_with("fn ") {
        functions.push(item.to_string());
    } else if !item.is_empty() {
        statements.push(item.to_string());
    }
}

fn classify_remaining(item: &str, functions: &mut Vec<String>, statements: &mut Vec<String>) {
    if (item.starts_with("pub fn ") || item.starts_with("fn ")) && item.ends_with('}') {
        functions.push(item.to_string());
    } else if !item.is_empty() {
        statements.push(item.to_string());
    }
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
    include!("tests.inc");
}
