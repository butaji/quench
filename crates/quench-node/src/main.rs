#![cfg(not(test))]

use quench_node::run::{
    eval_script_with_exec_argv, run_script_with_exec_argv, RunOutcome, EXTERNALIZE_STRINGS_FLAG,
};
use quench_runtime::vm::OutputSink;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self, ExitCode},
};
use walkdir::WalkDir;

fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let worker = std::thread::Builder::new()
        .name("quench-node".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(|| {
            let result = run_cli().map_err(|error| error.to_string());
            quench_runtime::execution_trace::emit();
            result
        })?;
    match worker.join() {
        Ok(Ok(status)) => Ok(status),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => {
            eprintln!("quench-node worker panicked");
            process::exit(1);
        }
    }
}

fn run_cli() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mode_index = args
        .iter()
        .position(|arg| {
            !arg.starts_with("--experimental-")
                && !arg.starts_with("--network-family-autoselection")
                && !arg.starts_with("--title=")
                && arg != EXTERNALIZE_STRINGS_FLAG
        })
        .unwrap_or(args.len());
    let exec_argv = if mode_index == 0 {
        quench_node::modules::process::inherited_exec_argv()
    } else {
        args[..mode_index].to_vec()
    };
    match args.get(mode_index).map(String::as_str) {
        Some("--help") | Some("-h") => {
            println!("quench-node [-e CODE|SCRIPT]");
            Ok(ExitCode::SUCCESS)
        }
        Some("--version") | Some("-v") => {
            println!("v22.0.0");
            Ok(ExitCode::SUCCESS)
        }
        Some("-e") | Some("--eval") => {
            let source = args.get(mode_index + 1).map_or("", String::as_str);
            let sink: OutputSink = std::sync::Arc::new(|chunk| print!("{chunk}"));
            complete(eval_script_with_exec_argv(source, sink, false, &exec_argv))
        }
        Some("--stage") => run_directory(
            &PathBuf::from(format!(
                "tests/node-compat/stage-{}",
                args.get(mode_index + 1).map(String::as_str).unwrap_or("0")
            )),
            &exec_argv,
        ),
        Some("--test-dir") | Some("--reuse-dir") => run_directory(
            &PathBuf::from(
                args.get(mode_index + 1)
                    .cloned()
                    .unwrap_or_else(|| "tests/node-compat".into()),
            ),
            &exec_argv,
        ),
        Some(path) => run_file(Path::new(path), &args[mode_index + 1..], &exec_argv),
        None => {
            let sink: OutputSink = std::sync::Arc::new(|chunk| print!("{chunk}"));
            complete(eval_script_with_exec_argv("", sink, false, &exec_argv))
        }
    }
}

fn run_file(
    path: &Path,
    script_args: &[String],
    exec_argv: &[String],
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let sink: OutputSink = std::sync::Arc::new(|chunk| print!("{chunk}"));
    complete(run_script_with_exec_argv(
        path,
        script_args,
        exec_argv,
        &source,
        sink,
    ))
}

fn complete(outcome: RunOutcome) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match outcome.error {
        Some(error) => Err(error.into()),
        None => Ok(ExitCode::from(outcome.exit_code as u8)),
    }
}

fn run_directory(
    dir: &PathBuf,
    exec_argv: &[String],
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    if dir.is_file() {
        return run_file(dir, &[], exec_argv);
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
            match run_file(entry.path(), &[], exec_argv) {
                Ok(status) if status == ExitCode::SUCCESS => {
                    println!("ok {}", entry.path().display())
                }
                Ok(_) => {
                    failed += 1;
                    eprintln!(
                        "not ok {}: script exited unsuccessfully",
                        entry.path().display()
                    );
                }
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
        Ok(ExitCode::SUCCESS)
    } else {
        Err("Quench harness failures".into())
    }
}
