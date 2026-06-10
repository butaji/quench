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
pub enum CompilerCmd {
    Compile { input: String, output: Option<String> },
    Run { input: String },
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
                println!("TuiBridge v{}", VERSION);
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
                    result.compiler_cmd = Some(CompilerCmd::Run {
                        input: input.clone(),
                    });
                    result.interactive = false;
                }
                i += 2;
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
    println!("TuiBridge v{}", VERSION);
    println!();
    println!("Usage: tuibridge [OPTIONS] [SCRIPT]");
    println!();
    println!("Options:");
    println!("  --help, -h     Show this help");
    println!("  --version, -v  Show version");
    println!("  --bundle FILE  Load bundled JS from FILE");
    println!("  --eval CODE    Execute CODE");
    println!("  --watch PATH   Watch for file changes and hot reload");
    println!("  --hot          Enable hot reload mode (shortcut for --watch .)");
    println!("  --prop KEY=VAL Pass a prop to the JS runtime (useBridge().config)");
    #[cfg(feature = "compiler")]
    {
        println!("  --compile FILE Compile TSX to TuiBridge JS");
        println!("  --run FILE     Compile and run TSX file");
        println!("  -o, --out FILE Output file for compiled JS");
    }
    println!();
    println!("Examples:");
    println!("  tuibridge --bundle plugins/app.tsx");
    println!("  tuibridge --hot examples/counter.js");
    println!("  tuibridge --watch plugins examples/app.js");
    #[cfg(feature = "compiler")]
    {
        println!();
        println!("Compiler (requires --features compiler):");
        println!("  tuibridge --compile mod.tsx -o mod-tb.js");
        println!("  tuibridge --run mod.tsx");
    }
}

/// Execute compiler command
#[cfg(feature = "compiler")]
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
        CompilerCmd::Run { input } => {
            match crate::compiler::compile_file(&input) {
                Ok(js) => {
                    let temp_file = format!("{}.compiled.js", input);
                    if let Err(e) = std::fs::write(&temp_file, &js) {
                        eprintln!("Failed to write temp file: {}", e);
                        std::process::exit(1);
                    }
                    let result = std::process::Command::new(std::env::current_exe().unwrap())
                        .arg(&temp_file)
                        .status();
                    let _ = std::fs::remove_file(&temp_file);
                    std::process::exit(result.map(|s| s.code().unwrap_or(0)).unwrap_or(1));
                }
                Err(e) => {
                    eprintln!("Compilation error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

#[cfg(not(feature = "compiler"))]
pub fn handle_compiler_cmd(_cmd: CompilerCmd) {
    eprintln!("Compiler feature not enabled. Build with --features compiler");
    std::process::exit(1);
}
