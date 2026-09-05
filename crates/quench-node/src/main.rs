#![cfg(not(test))]

use quench_node::run::eval_script;
use quench_runtime::vm::OutputSink;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};
use walkdir::WalkDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compact CallN trampolines, but remaining Slow CallMethod still rust-recurses.
    // Scheme-style fixtures (Earley-Boyer) overflow the default stack (~8MiB).
    const STACK: usize = 2048 * 1024 * 1024;
    let worker = std::thread::Builder::new()
        .name("quench-node".into())
        .stack_size(STACK)
        .spawn(|| {
            let result = run_cli().map_err(|error| error.to_string());
            quench_runtime::execution_trace::emit();
            result
        })?;
    match worker.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => {
            eprintln!("quench-node worker panicked");
            process::exit(1);
        }
    }
}

fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mode_index = args
        .iter()
        .position(|arg| {
                !arg.starts_with("--experimental-")
                && !arg.starts_with("--network-family-autoselection")
                && !arg.starts_with("--title=")
        })
        .unwrap_or(args.len());
    match args.get(mode_index).map(String::as_str) {
        Some("--help") | Some("-h") => {
            println!("quench-node [-e CODE|SCRIPT]");
            Ok(())
        }
        Some("--version") | Some("-v") => {
            println!("v22.0.0");
            Ok(())
        }
        Some("-e") | Some("--eval") => {
            let source = args.get(mode_index + 1).map_or("", String::as_str);
            let sink: OutputSink = std::sync::Arc::new(|line| println!("{line}"));
            match eval_script(source, sink).error {
                Some(error) => Err(error.into()),
                None => Ok(()),
            }
        }
        Some("--stage") => run_directory(&PathBuf::from(format!(
            "tests/node-compat/stage-{}",
            args.get(mode_index + 1).map(String::as_str).unwrap_or("0")
        ))),
        Some("--test-dir") | Some("--reuse-dir") => run_directory(&PathBuf::from(
            args.get(mode_index + 1)
                .cloned()
                .unwrap_or_else(|| "tests/node-compat".into()),
        )),
        Some(path) => run_file(Path::new(path), &args[mode_index + 1..]),
        None => {
            let sink: OutputSink = std::sync::Arc::new(|line| println!("{line}"));
            match eval_script("", sink).error {
                Some(error) => Err(error.into()),
                None => Ok(()),
            }
        }
    }
}

fn run_file(path: &Path, _script_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let sink: OutputSink = std::sync::Arc::new(|line| println!("{line}"));
    // Script files run as global programs, matching the CLI's ordinary file semantics.
    let outcome = eval_script(&source, sink);
    match outcome.error {
        Some(error) => Err(error.into()),
        None if outcome.exit_code == 0 => Ok(()),
        None => Err(format!("script exited with status {}", outcome.exit_code).into()),
    }
}

fn run_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if dir.is_file() {
        return run_file(dir, &[]);
    }
    let mut failed = 0;
    let mut total = 0;
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e == "js" || e == "mjs")
        {
            total += 1;
            match run_file(entry.path(), &[]) {
                Ok(()) => println!("ok {}", entry.path().display()),
                Err(error) => {
                    failed += 1;
                    eprintln!("not ok {}: {error:?}", entry.path().display());
                }
            }
        }
    }
    println!("{total} tests, {} passed, {failed} failed", total - failed);
    if total == 0 {
        Err("Quench harness found no JavaScript tests".into())
    } else if failed == 0 {
        Ok(())
    } else {
        Err("Quench harness failures".into())
    }
}
