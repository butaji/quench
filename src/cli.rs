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

/// What a flag expects.
enum FlagAction {
    /// Takes nothing (boolean flag)
    Bool,
    /// Takes the next argument as a path/value
    TakesValue,
    /// Takes the next argument as KEY=VAL
    TakesKV,
}

const FLAGS: &[(&str, FlagAction)] = &[
    ("--help", FlagAction::Bool),
    ("-h", FlagAction::Bool),
    ("--version", FlagAction::Bool),
    ("-v", FlagAction::Bool),
    ("--compile", FlagAction::TakesValue),
    ("--run", FlagAction::TakesValue),
    ("--watch", FlagAction::TakesValue),
    ("-w", FlagAction::TakesValue),
    ("--hot", FlagAction::Bool),
    ("--prop", FlagAction::TakesKV),
    ("--bundle", FlagAction::TakesValue),
    ("-b", FlagAction::TakesValue),
    ("--eval", FlagAction::TakesValue),
    ("-e", FlagAction::TakesValue),
];

/// Parse command line arguments using a single-pass flag table.
pub fn parse_args(args: &[String]) -> CliArgs {
    let mut result = CliArgs {
        script: None,
        js_code: None,
        interactive: true,
        watch_path: None,
        bridge_config: BridgeConfig::default(),
        compiler_cmd: None,
    };

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();

        if let Some(action) = FLAGS.iter().find(|(f, _)| *f == arg).map(|(_, a)| a) {
            match action {
                FlagAction::Bool => {
                    if let Some(new_i) = handle_bool_flag(arg, &mut result) {
                        i = new_i;
                        continue;
                    }
                }
                FlagAction::TakesValue => {
                    if let Some(new_i) = handle_value_flag(arg, args, i, &mut result) {
                        i = new_i;
                        continue;
                    }
                }
                FlagAction::TakesKV => {
                    if let Some(new_i) = handle_kv_flag(args, i, &mut result) {
                        i = new_i;
                        continue;
                    }
                }
            }
        } else if arg == "run" {
            if let Some(input) = args.get(i + 1) {
                result.compiler_cmd = Some(CompilerCmd::CompileInMemory {
                    input: input.clone(),
                });
                result.interactive = true;
            }
            i += 2;
            continue;
        } else if !arg.starts_with('-') && result.script.is_none() {
            handle_positional(arg, &mut result);
            i += 1;
            continue;
        }

        i += 1;
    }

    if result.script.is_some() {
        result.interactive = true;
    }

    result
}

fn handle_bool_flag(flag: &str, result: &mut CliArgs) -> Option<usize> {
    match flag {
        "--help" | "-h" => {
            print_help();
            std::process::exit(0);
        }
        "--version" | "-v" => {
            println!("Quench v{}", VERSION);
            std::process::exit(0);
        }
        "--hot" => {
            result.watch_path = Some(".".to_string());
            Some(0) // consumed, caller adds 1
        }
        _ => None,
    }
}

fn handle_value_flag(
    flag: &str,
    args: &[String],
    i: usize,
    result: &mut CliArgs,
) -> Option<usize> {
    let value = args.get(i + 1)?;
    match flag {
        "--compile" => {
            let output = args
                .iter()
                .position(|a| a == "-o" || a == "--out")
                .and_then(|j| args.get(j + 1).cloned());
            result.compiler_cmd = Some(CompilerCmd::Compile {
                input: value.clone(),
                output,
            });
            result.interactive = false;
        }
        "--run" => {
            result.compiler_cmd = Some(CompilerCmd::CompileInMemory {
                input: value.clone(),
            });
            result.interactive = true;
        }
        "--watch" | "-w" => {
            result.watch_path = Some(value.clone());
        }
        "--bundle" | "-b" => {
            result.script = Some(value.clone());
        }
        "--eval" | "-e" => {
            result.js_code = Some(value.clone());
        }
        _ => return None,
    }
    Some(i + 2)
}

fn handle_kv_flag(args: &[String], i: usize, result: &mut CliArgs) -> Option<usize> {
    let kv = args.get(i + 1)?;
    if let Some((key, val)) = kv.split_once('=') {
        result.bridge_config.props.insert(key.to_string(), val.to_string());
    }
    Some(i + 2)
}

fn handle_positional(path: &str, result: &mut CliArgs) {
    if path.ends_with(".tsx") || path.ends_with(".ts") || path.ends_with(".jsx") {
        result.compiler_cmd = Some(CompilerCmd::CompileInMemory {
            input: path.to_string(),
        });
    } else {
        result.script = Some(path.to_string());
    }
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
