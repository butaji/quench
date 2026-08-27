//! `quench-node-test`'s single CLI entry point. Runs one
//! JavaScript file through the host and prints the outcome.
//!
//! Usage: `cargo run -p quench-node-test --bin run -- <file.js>`

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use quench_node_test::runner::NodeTestRunner;

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: cargo run -p quench-node-test --bin run -- <file.js>");
        return ExitCode::from(2);
    };
    let argv = std::env::args_os()
        .skip(2)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let child_mode = std::env::var_os("QUENCH_CHILD_RUNNER").is_some();
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink_capture = Arc::clone(&captured);
    let sink: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line| {
        if let Ok(mut lines) = sink_capture.lock() { lines.push(line.to_string()); }
    });
    let mut runner = NodeTestRunner::new().with_output_sink(sink);
    let outcome = runner.run_file_with_args(&path, argv);
    if child_mode {
        let lines = captured.lock().map(|lines| lines.clone()).unwrap_or_default();
        for line in &lines { println!("{line}"); }
        let todo = lines.iter().filter(|line| line.contains("# TODO") || line.contains("# todo")).count();
        let pass = lines.iter().filter(|line| line.starts_with("ok ") && !line.contains("# TODO") && !line.contains("# todo")).count();
        let fail = lines.iter().filter(|line| line.starts_with("not ok ")).count();
        println!("1..{}\n# tests {}\n# pass {}\n# fail {}\n# cancelled 0\n# todo {}", pass + fail + todo, pass + fail + todo, pass, fail, todo);
    }
    match outcome {
        quench_node_test::NodeOutcome::Pass => {
            if std::env::var_os("QUENCH_CHILD_RUNNER").is_none() {
                println!("PASS {}", path.display());
            }
            ExitCode::SUCCESS
        }
        quench_node_test::NodeOutcome::Fail { reason } => {
            if reason.starts_with("read ") {
                eprintln!("Cannot find module '{}'", path.display());
            } else {
                eprintln!("FAIL {}: {reason}", path.display());
            }
            ExitCode::from(1)
        }
        quench_node_test::NodeOutcome::Skip { reason } => {
            println!("SKIP {}: {reason}", path.display());
            ExitCode::from(0)
        }
    }
}
