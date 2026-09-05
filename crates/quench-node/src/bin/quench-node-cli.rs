//! `quench-node` — Node.js-style command line: `quench-node <script.js> [args]`.
//!
//! Installs the Node-API host, runs the script as a CJS module through
//! the event loop, and exits with the resolved exit code — mirroring
//! how real npm apps are launched under a Node binary.

use std::path::Path;
use std::process::ExitCode;

const VERSION: &str = "v22.0.0";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("{VERSION}");
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return ExitCode::SUCCESS;
    }
    if args.first().is_some_and(|arg| arg == "-e" || arg == "--eval") {
        let source = args.get(1).cloned().unwrap_or_default();
        let outcome =
            quench_node::run::eval_script(&source, std::sync::Arc::new(|line| println!("{line}")));
        if let Some(error) = &outcome.error {
            eprintln!("{error}");
        }
        return ExitCode::from(outcome.exit_code.clamp(0, 255) as u8);
    }
    let Some(script_index) = args.iter().position(|arg| !arg.starts_with('-')) else {
        print_usage();
        return ExitCode::from(64);
    };
    let script = &args[script_index];
    let path = Path::new(script);
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("Cannot find module '{script}': {error}");
            return ExitCode::from(66);
        }
    };
    let outcome = quench_node::run::run_script_with_exec_argv(
        path,
        &args[script_index + 1..],
        &args[..script_index],
        &source,
        std::sync::Arc::new(|line| println!("{line}")),
    );
    if let Some(error) = &outcome.error {
        eprintln!("{error}");
    }
    ExitCode::from(outcome.exit_code.clamp(0, 255) as u8)
}

fn print_usage() {
    eprintln!(
        "usage: quench-node [options] <script.js> [args]\n\
         options:\n  \
         -v, --version   print the runtime version and exit\n  \
         -h, --help      print this help and exit"
    );
}
