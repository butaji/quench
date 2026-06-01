//! Source file generation
//!
//! allow:complexity,too_many_lines

use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use anyhow::{Context, Result};

use crate::commands::build::{ComponentEntry, GeneratedFile, IslandEntry, RouteEntry};
use crate::transpile::hir::QuoteCodegen;
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

/// Generate source files from TypeScript using QuoteCodegen
pub fn generate_all(files: &[PathBuf]) -> Result<Vec<GeneratedFile>, anyhow::Error> {
    let mut generated = Vec::new();
    let parser = TsParser::new();
    let codegen = QuoteCodegen::default();

    for file in files {
        let relative = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("mod.rs")
            .replace(".tsx", ".rs")
            .replace(".ts", ".rs");

        // Parse TypeScript to HIR
        let source = std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        let module = match parser.parse_source(&source) {
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

        // Generate Rust using QuoteCodegen
        // Extract both Stmt items and Decl items (functions, variables) for codegen
        let stmts: Vec<_> = module.items.into_iter()
            .filter_map(|item| match item {
                crate::transpile::hir::ModuleItem::Stmt(s) => Some(s),
                crate::transpile::hir::ModuleItem::Decl(d) => match d {
                    crate::transpile::hir::Decl::Function(func) => {
                        Some(crate::transpile::hir::Stmt::FunctionDecl(func))
                    }
                    crate::transpile::hir::Decl::Variable(var) => {
                        Some(crate::transpile::hir::Stmt::Variable(var))
                    }
                    _ => None, // Skip type and class declarations for now
                },
                _ => None,
            })
            .collect();

        let tokens = codegen.gen_module(&stmts);
        let rust_code = tokens.to_string();

        generated.push(GeneratedFile {
            path: PathBuf::from(format!("src/gen/{}", relative)),
            content: format!("// Generated from {}\n\n{}", file.display(), rust_code),
        });
    }

    Ok(generated)
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

/// Generate main.rs
/// For single-file builds, pass the module name to generate a simple entry point
pub fn generate_main(source_file: Option<&Path>) -> String {
    let mut output = String::new();
    output.push_str("//! Auto-generated main\n\n");

    if source_file.is_some() {
        // Single-file build: call the crate's main function
        // The transpiled code is directly in lib.rs, which has a main() function
        output.push_str("// For single-file builds, lib.rs IS the main module\n");
        output.push_str("use crate::main as run_main;\n\n");
        output.push_str("fn main() {\n");
        output.push_str("    run_main();\n");
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
    if !current.trim().is_empty() {
        let item = current.trim().to_string();
        if item.starts_with("pub fn ") || item.starts_with("fn ") {
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
