//! runts — Fresh/Preact to Native Rust Compiler
//!
//! A CLI tool that compiles Fresh/Preact TypeScript/TSX to native Rust binaries.

#![allow(dead_code)]

mod cli;
mod commands;
mod config;
mod runtime;
mod transpile;
mod util;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::info;
use transpile::hir;

use cli::Cli;

fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();
    execute(cli)
}

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_env_filter("runts=info")
        .try_init();
}

fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        cli::Commands::Eval { expr } => run_eval(&expr),
        cli::Commands::Codegen { source, expr } => run_codegen(source, expr),
        cli::Commands::Init { name } => run_init(name),
        cli::Commands::Dev { path } => run_dev(path),
        cli::Commands::Build {
            path,
            release,
            no_compile,
        } => run_build(path, release, no_compile),
        cli::Commands::Transpile { path, output: _ } => run_transpile(path),
        cli::Commands::Add {
            component_type,
            name,
            path,
        } => run_add(component_type.into(), name, path),
    }
}

fn run_init(name: String) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(commands::run_init(name, None))
}

fn run_dev(path: PathBuf) -> Result<()> {
    info!("Starting development server...");
    let config = config::Config::load_from_path(&path)?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(commands::run_dev_server(&config, 8000))
}

fn run_build(path: PathBuf, release: bool, no_compile: bool) -> Result<()> {
    info!("Building for production...");
    let config = config::Config::load_from_path(&path)?;
    let rt = tokio::runtime::Runtime::new()?;
    let path_str = path.to_string_lossy().to_string();

    if no_compile {
        transpile_only(&config, &path_str, &rt)
    } else {
        full_build(&config, &path_str, release, &rt)
    }
}

fn transpile_only(config: &config::Config, path: &str, rt: &tokio::runtime::Runtime) -> Result<()> {
    let path_buf = std::path::PathBuf::from(path);
    let result = rt.block_on(commands::run_build(config, path_buf))?;
    info!("Transpilation complete!");
    info_build_summary(&result);
    info!("Run `cargo build --release` to compile.");
    Ok(())
}

fn full_build(
    config: &config::Config,
    path: &str,
    release: bool,
    rt: &tokio::runtime::Runtime,
) -> Result<()> {
    let path_buf = std::path::PathBuf::from(path);
    let result = rt.block_on(commands::build::run_full_build(config, path_buf, release))?;
    info!("Build complete!");
    if let Some(binary) = &result.binary_path {
        info!("  Binary: {:?}", binary);
    }
    if let Some(size) = result.binary_path_size {
        info!("  Size: {:.2} KB", size as f64 / 1024.0);
    }
    Ok(())
}

fn run_transpile(path: PathBuf) -> Result<()> {
    info!("Transpiling TypeScript to Rust...");
    let config = config::Config::load_from_path(&path)?;
    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(commands::run_build(&config, path))?;
    info!("Transpilation complete!");
    info_build_summary(&result);
    Ok(())
}

fn run_eval(expr: &str) -> Result<()> {
    let parser = transpile::TsParser::new();
    let interpreter = runtime::interpreter::Interpreter::new();
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        println!("undefined");
        return Ok(());
    }
    // Check if it's a statement (not an expression)
    if is_statement_keyword(trimmed) {
        match parser.parse_source(trimmed) {
            Ok(module) => {
                // Analysis passes run during parsing
                let result = interpreter.eval_module_stmts(&module);
                print_result(&result);
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("Parse error: {}", e)),
        }
    } else {
        // Wrap as expression
        let source = format!("const __result = {};", trimmed);
        match parser.parse_source(&source) {
            Ok(module) => {
                // Analysis passes run during parsing
                let result = interpreter.eval_module(&module);
                print_result(&result);
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("Parse error: {}", e)),
        }
    }
}
fn prepare_source(stmt: &str) -> String {
    let trimmed = stmt.trim();
    if is_statement_keyword(trimmed) {
        stmt.to_string()
    } else {
        format!("const __result = {};", stmt)
    }
}
fn is_statement_keyword(s: &str) -> bool {
    let kws = [
        "if ",
        "for ",
        "while ",
        "return ",
        "throw ",
        "try ",
        "switch ",
        "do ",
        "let ",
        "const ",
        "var ",
        "function ",
        "class ",
        "{",
    ];
    kws.iter().any(|k| s.starts_with(k))
}
fn print_result(result: &str) {
    if result.is_empty() {
        println!("undefined");
    } else {
        println!("{}", result);
    }
}

/// Run in-memory Rust codegen from TypeScript using QuoteCodegen
fn run_codegen(source: Option<String>, expr: Option<String>) -> Result<()> {
    use transpile::hir::QuoteCodegen;
    use transpile::hir::Stmt;
    
    let input = expr.or(source).unwrap_or_default();
    if input.is_empty() {
        println!("// No input provided");
        return Ok(());
    }
    
    // Parse TypeScript to HIR
    let parser = transpile::TsParser::new();
    let module = parser.parse_source(&input)?;
    
    // Extract statements from module items
    let stmts: Vec<Stmt> = module.items.into_iter().filter_map(|item| {
        match item {
            transpile::hir::ModuleItem::Stmt(s) => Some(s),
            _ => None,
        }
    }).collect();
    
    // Generate Rust using QuoteCodegen (in-memory, no files)
    let cg = QuoteCodegen::default();
    let tokens = cg.gen_module(&stmts);
    
    // Output the generated Rust code
    println!("{}", tokens);
    
    Ok(())
}

fn run_add(
    component_type: commands::add::ComponentType,
    name: String,
    path: PathBuf,
) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(commands::run_add(component_type, name, Some(path)))
}

fn info_build_summary(_result: &commands::build::BuildResult) {
    // Build summary is printed by the build command
}
