//! `quench-node-test`'s single CLI entry point. Runs one
//! JavaScript file through the host and prints the outcome.
//!
//! Usage: `cargo run -p quench-node-test --bin run -- <file.js>`

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use quench_node_test::runner::NodeTestRunner;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let input_type = arguments
        .windows(2)
        .find_map(|pair| (pair[0] == "--input-type").then(|| pair[1].as_str()));
    if let Some(index) = arguments
        .iter()
        .position(|arg| arg == "--eval" || arg == "-e")
    {
        let source = arguments
            .get(index + 1)
            .map(String::as_str)
            .unwrap_or_default();
        let source = if input_type == Some("module") {
            quench_node::esm_imports::transform_esm_imports(source)
        } else {
            source.to_string()
        };
        let outcome = quench_node::run::eval_script(&source, Arc::new(|line| println!("{line}")));
        if let Some(error) = outcome.error {
            eprintln!("{error}");
        }
        return ExitCode::from(outcome.exit_code.clamp(0, 255) as u8);
    }
    let Some(path) = arguments.first().map(PathBuf::from) else {
        eprintln!("usage: cargo run -p quench-node-test --bin run -- <file.js>");
        return ExitCode::from(2);
    };
    let argv = arguments.into_iter().skip(1).collect();
    let child_mode = std::env::var_os("QUENCH_CHILD_RUNNER").is_some();
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink_capture = Arc::clone(&captured);
    let sink: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line| {
        if let Ok(mut lines) = sink_capture.lock() {
            lines.push(line.to_string());
        }
    });
    let mut runner = NodeTestRunner::new().with_output_sink(sink);
    let outcome = runner.run_file_with_args(&path, argv);
    if child_mode {
        let lines = captured
            .lock()
            .map(|lines| lines.clone())
            .unwrap_or_default();
        for line in &lines {
            println!("{line}");
        }
        let todo = lines
            .iter()
            .filter(|line| line.contains("# TODO") || line.contains("# todo"))
            .count();
        let cancelled = lines
            .iter()
            .filter(|line| {
                line.contains("# CANCELLED")
                    || line.contains("# cancelled")
                    || line.contains("# CANCELED")
            })
            .count();
        let pass = lines
            .iter()
            .filter(|line| {
                line.starts_with("ok ")
                    && !line.contains("# TODO")
                    && !line.contains("# todo")
                    && !line.contains("# CANCELLED")
                    && !line.contains("# CANCELED")
                    && !line.contains("# cancelled")
            })
            .count();
        let fail = lines
            .iter()
            .filter(|line| line.starts_with("not ok "))
            .count();
        if pass + fail + todo + cancelled > 0 {
            println!(
                "1..{}\n# tests {}\n# pass {}\n# fail {}\n# cancelled {}\n# todo {}",
                pass + fail + todo + cancelled,
                pass + fail + todo + cancelled,
                pass,
                fail,
                cancelled,
                todo
            );
        }
        if fail != 0 || cancelled != 0 {
            return ExitCode::from(1);
        }
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
