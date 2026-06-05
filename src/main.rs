//! runts — Fresh/Preact to Native Rust Compiler
//!
//! A CLI tool that compiles Fresh/Preact TypeScript/TSX to native Rust binaries.

mod cli;
mod commands;
mod config;
mod hir_runtime;
mod plugin;
mod runtime;
mod transpile;
mod util;

use anyhow::Result;
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use tracing::info;

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
    let cmd = &cli.command;
    run_simple_cmd(cmd)?;
    run_complex_cmd(cmd)
}

fn run_simple_cmd(cmd: &cli::Commands) -> Result<()> {
    match cmd {
        cli::Commands::Eval { expr } => run_eval(expr),
        cli::Commands::Init { name } => run_init(name.clone()),
        cli::Commands::HirRender { path } => run_hir_render(path.clone()),
        cli::Commands::InspectHir { path } => run_inspect_hir(path.clone()),
        _ => Ok(()),
    }
}

fn run_complex_cmd(cmd: &cli::Commands) -> Result<()> {
    match cmd {
        cli::Commands::Codegen { source, expr } => run_codegen(source.clone(), expr.clone()),
        cli::Commands::Dev { path, plugin } => run_dev(path.clone(), plugin),
        cli::Commands::Build { path, plugin, release, no_compile } => {
            run_build(path.clone(), plugin.clone(), *release, *no_compile)
        }
        cli::Commands::Transpile { path, output } => run_transpile(path.clone(), output.clone()),
        cli::Commands::Add { component_type, name, path } => {
            run_add((*component_type).into(), name.clone(), path.clone())
        }
        _ => Ok(()),
    }
}

fn run_init(name: String) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(commands::run_init(name, None))
}

fn run_hir_render(path: PathBuf) -> Result<()> {
    let source = std::fs::read_to_string(&path)?;
    // Read terminal size from environment variables
    let cols: u16 = std::env::var("COLS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80);
    let lines: u16 = std::env::var("LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);
    let output = hir_runtime::render_tsx(&source, cols, lines)
        .map_err(|e| anyhow::anyhow!("HIR render error: {e:?}"))?;
    print!("{output}");
    Ok(())
}

fn run_dev(path: PathBuf, plugin_name: &str) -> Result<()> {
    info!("Starting development server with plugin: {}", plugin_name);
    let config = config::Config::load_from_path(&path)?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(commands::run_dev_server(&config, path, plugin_name.to_string()))
}

fn run_build(path: PathBuf, plugin: Option<String>, release: bool, no_compile: bool) -> Result<()> {
    let plugin_name = plugin.as_deref().unwrap_or("none");
    info!("Building for production with plugin: {}", plugin_name);
    let config = config::Config::load_from_path(&path)?;
    let rt = tokio::runtime::Runtime::new()?;

    if no_compile {
        let path_str = path.to_string_lossy().to_string();
        transpile_only(&config, &path_str, &rt)
    } else if plugin_name != "none" {
        // Use plugin-based ephemeral build
        let result = rt.block_on(commands::build::run_plugin_build(&config, path, plugin_name.to_string(), release))?;
        info_plugin_build_result(&result);
        Ok(())
    } else {
        let path_str = path.to_string_lossy().to_string();
        full_build(&config, &path_str, release, &rt)
    }
}

fn info_plugin_build_result(result: &commands::build::BuildResult) {
    info!("Build complete!");
    if let Some(binary) = &result.binary_path {
        info!("  Binary: {:?}", binary);
    }
    if let Some(size) = result.binary_path_size {
        info!("  Size: {:.2} KB", size as f64 / 1024.0);
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

fn run_transpile(path: PathBuf, output: Option<PathBuf>) -> Result<()> {
    info!("Transpiling TypeScript to Rust...");
    let config = config::Config::load_from_path(&path)?;
    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(commands::run_build(&config, path))?;
    info!("Transpilation complete!");
    info_build_summary(&result);
    if let Some(out_path) = output {
        for file in &result.generated_files {
            let dest = out_path.join(&file.path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest, &file.content)?;
        }
        info!("Output written to: {:?}", out_path);
    }
    Ok(())
}

fn run_eval(expr: &str) -> Result<()> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        println!("undefined");
        return Ok(());
    }
    // QuickJsRuntime::eval handles its own console-shim wrapping
    // (expression-form vs. statement-form detection) and host-fn
    // registration for __runts_stderr__. We pass the raw user code
    // directly; a previous `prepare_source` layer added a redundant
    // `const __runts_val = X; __runts_val` wrap that interacted badly with
    // the QuickJS-side shim and made multi-statement input return
    // `undefined` (the value of the var-decl) instead of the last
    // expression.
    let js = runtime::quickjs::QuickJsRuntime::new();
    match js.eval(trimmed) {
        Ok(result) => {
            print_result(&result);
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("Failed to evaluate '{}': {}", expr, e)),
    }
}

/// Parse a single .tsx/.ts file and dump the HIR as
/// pretty-printed JSON. Used by the JSX codegen work
/// to verify what the parser emits — we used to
/// discover the JSON shape only by reading the
/// parser source.
fn run_inspect_hir(path: PathBuf) -> Result<()> {
    let source = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("read error for {}: {e}", path.display()))?;
    let is_tsx = path.extension().is_some_and(|e| e == "tsx" || e == "ts");
    let module = transpile::parser::parse_source(&source, is_tsx)
        .map_err(|e| anyhow::anyhow!("parse error: {e}"))?;
    let json = serde_json::to_string_pretty(&module)
        .map_err(|e| anyhow::anyhow!("serialize error: {e}"))?;
    println!("{json}");
    Ok(())
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

    // Parse TypeScript to HIR. The parser must know whether JSX is allowed:
    // .tsx files contain JSX, .ts files do not. We detect from the file
    // extension when --source was used with a file path; for --expr or for
    // stdin input, default to non-JSX (caller can re-run with --source if
    // they need JSX).
    let parser = transpile::TsParser::new();
    let module = detect_and_parse(&parser, &input)?;

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

/// Parse `input` as TypeScript, picking JSX mode based on a `.tsx`/`.ts`/`.jsx`
/// file extension. `input` is the literal value passed via `--expr`, or the
/// file contents read by `--source`. For the file-contents case the extension
/// is recoverable from the first newline-delimited token (we check the
/// raw arg from the option, not the read contents — see `cli::Commands::Codegen`).
fn detect_and_parse(
    parser: &transpile::TsParser,
    input: &str,
) -> Result<transpile::hir::Module> {
    use std::path::Path;
    let path = Path::new(input);
    // Only treat `input` as a path if it looks like one (no newlines, has
    // an extension, exists or could plausibly be a file). For inline expr
    // input the parse below will try parse_source, which is the right
    // behavior for non-JSX TS.
    if input.lines().count() == 1
        && path.extension().is_some()
        && (input.ends_with(".tsx") || input.ends_with(".jsx") || input.ends_with(".ts"))
    {
        // Read the file and parse with the right mode.
        let contents = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read source file '{}': {}", input, e))?;
        if input.ends_with(".tsx") || input.ends_with(".jsx") {
            return parser.parse_tsx(&contents).map_err(Into::into);
        }
        return parser.parse_source(&contents).map_err(Into::into);
    }
    // Inline expression / snippet: no JSX support by default.
    parser.parse_source(input).map_err(Into::into)
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
