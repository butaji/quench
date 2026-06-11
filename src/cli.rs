//! CLI argument parsing and help text
//!
//! Handles --help, --version, --bundle, --eval, --compile, --run, --watch, --hot, --prop

use crate::bridge_config::BridgeConfig;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Parsed CLI arguments
pub struct CliArgs {
    pub script: Option<String>,
    pub js_code: Option<String>,
    pub interactive: bool,
    pub watch_path: Option<String>,
    pub bridge_config: BridgeConfig,
    pub compiler_cmd: Option<CompilerCmd>,
}

/// Compiler subcommands
#[derive(Clone)]
pub enum CompilerCmd {
    Compile { input: String, output: Option<String> },
    CompileInMemory { input: String },
}

/// Parse command line arguments
pub fn parse_args(args: &[String]) -> CliArgs {
    let mut result = CliArgs {
        script: None,
        js_code: None,
        interactive: true,
        watch_path: None,
        bridge_config: BridgeConfig::default(),
        compiler_cmd: None,
    };

    // Skip the program name (args[0])
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--version" | "-v" => {
                println!("Quench v{}", VERSION);
                std::process::exit(0);
            }
            "--compile" => {
                if let Some(input) = args.get(i + 1) {
                    let output = args.iter()
                        .position(|a| a == "-o" || a == "--out")
                        .and_then(|j| args.get(j + 1).cloned());
                    result.compiler_cmd = Some(CompilerCmd::Compile {
                        input: input.clone(),
                        output,
                    });
                    result.interactive = false;
                }
                i += 2;
            }
            "--run" => {
                if let Some(input) = args.get(i + 1) {
                    // --run always in-memory compiles (no temp file written)
                    result.compiler_cmd = Some(CompilerCmd::CompileInMemory {
                        input: input.clone(),
                    });
                    result.interactive = true;
                }
                i += 2;
            }
            "run" => {
                // `quench run <file>` subcommand
                if let Some(input) = args.get(i + 1) {
                    result.compiler_cmd = Some(CompilerCmd::CompileInMemory {
                        input: input.clone(),
                    });
                    result.interactive = true;
                }
                i += 2;
            }
            // Support direct TSX/TS file execution with in-memory compile
            arg if !arg.starts_with('-') && result.script.is_none() => {
                let path = arg;
                // Only compile files that need JSX transformation
                // Plain .js files can be loaded directly
                if path.ends_with(".tsx") || path.ends_with(".ts") || path.ends_with(".jsx") {
                    // Compile in-memory for JSX/TSX/TS files
                    result.compiler_cmd = Some(CompilerCmd::CompileInMemory {
                        input: path.to_string(),
                    });
                } else if path.ends_with(".js") {
                    // Plain JS files - load directly without compilation
                    result.script = Some(path.to_string());
                } else {
                    result.script = Some(path.to_string());
                }
                i += 1;
            }
            "--watch" | "-w" => {
                if let Some(path) = args.get(i + 1) {
                    result.watch_path = Some(path.clone());
                }
                i += 2;
            }
            "--hot" => {
                result.watch_path = Some(".".to_string());
                i += 1;
            }
            "--prop" => {
                if let Some(kv) = args.get(i + 1) {
                    if let Some((key, val)) = kv.split_once('=') {
                        result.bridge_config.props.insert(key.to_string(), val.to_string());
                    }
                }
                i += 2;
            }
            "--bundle" | "-b" => {
                if let Some(path) = args.get(i + 1) {
                    result.script = Some(path.clone());
                }
                i += 2;
            }
            "--eval" | "-e" => {
                if let Some(code) = args.get(i + 1) {
                    result.js_code = Some(code.clone());
                }
                i += 2;
            }
            arg if !arg.starts_with('-') && result.script.is_none() => {
                result.script = Some(arg.to_string());
                i += 1;
            }
            _ => i += 1,
        }
    }

    // Determine interactive mode
    if result.script.is_some() {
        result.interactive = true;
    }

    result
}

/// Print help text
pub fn print_help() {
    println!("Quench v{}", VERSION);
    println!();
    println!("Usage: quench [OPTIONS] [SCRIPT]");
    println!();
    println!("Options:");
    println!("  --help, -h     Show this help");
    println!("  --version, -v  Show version");
    println!("  --bundle FILE  Load bundled JS from FILE");
    println!("  --eval CODE    Execute CODE");
    println!("  --watch PATH   Watch for file changes and hot reload");
    println!("  --hot          Enable hot reload mode (shortcut for --watch .)");
    println!("  --prop KEY=VAL Pass a prop to the JS runtime (useBridge().config)");
    println!("  --compile FILE Compile TSX to Quench JS");
    println!("  --run FILE     Compile and run TSX file");
    println!("  -o, --out FILE Output file for compiled JS");
    println!();
    println!("Examples:");
    println!("  quench --bundle plugins/app.tsx");
    println!("  quench --hot examples/counter.js");
    println!("  quench --watch plugins examples/app.js");
    println!();
    println!("Compiler:");
    println!("  quench --compile mod.tsx -o mod-tb.js");
    println!("  quench run mod.tsx       # compile and run in-memory");
    println!("  quench mod.tsx           # auto-detect .tsx, in-memory compile");
}

/// Execute compiler command
pub fn handle_compiler_cmd(cmd: CompilerCmd) {
    match cmd {
        CompilerCmd::Compile { input, output } => {
            match crate::compiler::compile_file(&input) {
                Ok(js) => {
                    if let Some(out) = output {
                        if let Err(e) = std::fs::write(&out, &js) {
                            eprintln!("Failed to write output: {}", e);
                            std::process::exit(1);
                        }
                        println!("Compiled {} -> {}", input, out);
                    } else {
                        println!("{}", js);
                    }
                }
                Err(e) => {
                    eprintln!("Compilation error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        CompilerCmd::CompileInMemory { input: _ } => {
            // This is handled separately in main.rs
        }
    }
}

/// Compile TSX/TS file to JS string (for in-memory execution)
pub fn compile_in_memory(input: &str) -> Result<String, anyhow::Error> {
    crate::compiler::compile_file(input)
}
