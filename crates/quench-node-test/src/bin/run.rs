//! `quench-node-test`'s single CLI entry point. Runs one
//! JavaScript file through the host and prints the outcome.
//!
//! Usage: `cargo run -p quench-node-test --bin run -- <file.js>`

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use quench_node_test::runner::NodeTestRunner;

/// `node -pe` accepts a program (including declarations), then prints the
/// completion value of its final top-level expression.  Wrapping the whole
/// program in `console.log(...)` turns valid declaration programs into a
/// syntax error.  Preserve the program body and only wrap its final statement
/// expression; this keeps the CLI rule at the argument boundary rather than
/// teaching child-process callers about particular scripts.
fn wrap_combined_print_eval(source: &str) -> String {
    let mut quote = None;
    let mut escaped = false;
    let mut paren = 0usize;
    let mut bracket = 0usize;
    let mut brace = 0usize;
    let mut last_top_level_semicolon = None;
    for (index, ch) in source.char_indices() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            ';' if paren == 0 && bracket == 0 && brace == 0 => {
                last_top_level_semicolon = Some(index)
            }
            _ => {}
        }
    }
    if let Some(separator) = last_top_level_semicolon {
        let prefix = &source[..=separator];
        let tail = source[separator + 1..].trim();
        if !tail.is_empty() {
            return format!("{prefix}\nconsole.log({tail});");
        }
        return format!("{prefix}\nconsole.log(undefined);");
    }
    let trimmed = source.trim_start();
    if trimmed.starts_with("const ")
        || trimmed.starts_with("let ")
        || trimmed.starts_with("var ")
        || trimmed.starts_with("function ")
        || trimmed.starts_with("class ")
    {
        format!("{source}\nconsole.log(undefined);")
    } else {
        format!("console.log({source});")
    }
}

fn main() -> ExitCode {
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    if let Some(raw) = std::env::var_os("NODE_OPTIONS") {
        let raw = raw.to_string_lossy();
        let options = match quench_node_test::reader::parse_node_options(&raw) {
            Ok(options) => options,
            Err(_) => {
                eprintln!("{}: invalid NODE_OPTIONS", node_exec_path());
                return ExitCode::from(9);
            }
        };
        if let Some(disallowed) = options.iter().find(|option| node_option_disallowed(option)) {
            eprintln!(
                "{}: {disallowed} is not allowed in NODE_OPTIONS",
                node_exec_path()
            );
            return ExitCode::from(9);
        }
        let mut with_environment = options;
        with_environment.append(&mut arguments);
        arguments = with_environment;
    }
    // OpenSSL 3 aborts during startup when an explicit provider configuration
    // omits the default provider.  The Rust host does not delegate startup
    // to OpenSSL's CLI, but must preserve this process-level contract for
    // self-reexec children and ordinary command-line invocations.
    if openssl_config_has_no_default_provider(&arguments) {
        std::process::abort();
    }
    // A self-reexecuted compatibility child must honor the same simple CLI
    // probe as Node even when permission flags precede --version.  Keep this
    // at the Rust boundary so child_process does not special-case filenames
    // or project fixtures.
    if arguments
        .iter()
        .any(|arg| arg == "--version" || arg == "-v")
        && !arguments
            .iter()
            .any(|arg| arg.ends_with(".js") || arg.ends_with(".mjs") || arg.ends_with(".cjs"))
    {
        println!("v22.0.0");
        return ExitCode::SUCCESS;
    }
    let input_type = arguments
        .iter()
        .find_map(|arg| arg.strip_prefix("--input-type="))
        .or_else(|| {
            arguments
                .windows(2)
                .find_map(|pair| (pair[0] == "--input-type").then(|| pair[1].as_str()))
        });
    if let Some(index) = arguments.iter().position(|arg| {
        matches!(
            arg.as_str(),
            "--eval" | "-e" | "--print" | "-p" | "-pe" | "-ep"
        )
    }) {
        if arguments.iter().any(|arg| arg == "--enable-fips") {
            eprintln!("--enable-fips requires an active OpenSSL provider named \"fips\"");
            return ExitCode::from(1);
        }
        if arguments.iter().any(|arg| arg == "--force-fips") {
            eprintln!("--force-fips requires an active OpenSSL provider named \"fips\"");
            return ExitCode::from(1);
        }
        let source = arguments
            .get(index + 1)
            .map(String::as_str)
            .unwrap_or_default();
        // `-p/--print` evaluates its expression and writes the resulting
        // value. The compatibility runner shares this CLI with self-reexec
        // child processes, so make the print contract explicit at the Rust
        // boundary instead of asking each child-process caller to emulate it.
        let is_print = matches!(
            arguments.get(index).map(String::as_str),
            Some("--print" | "-p" | "-pe" | "-ep")
        );
        let source = if is_print {
            let combined_print_eval =
                matches!(arguments.get(index).map(String::as_str), Some("-pe"));
            if combined_print_eval {
                wrap_combined_print_eval(source)
            } else {
                format!("console.log({source});")
            }
        } else {
            source.to_string()
        };
        let source = format!(
            "{}{}",
            quench_node_test::reader::node_preload_program(&arguments),
            source
        );
        let source = if input_type == Some("module") {
            quench_node::esm_imports::transform_esm_imports(&source)
        } else {
            source
        };
        let source = if input_type == Some("module") && source.contains("await ") {
            format!("(async () => {{\n{source}\n}})();")
        } else {
            source
        };
        let child_mode = std::env::var_os("QUENCH_CHILD_RUNNER").is_some();
        let captured = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink_capture = Arc::clone(&captured);
        let sink: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line| {
            if let Ok(mut lines) = sink_capture.lock() {
                lines.push(line.to_string());
            }
        });
        let outcome = quench_node::run::eval_script_with_exec_argv(
            &source,
            sink,
            input_type == Some("module"),
            &arguments[..index],
        );
        if child_mode {
            if let Ok(lines) = captured.lock() {
                for line in lines.iter() {
                    print!("{line}");
                }
            }
        } else if let Ok(lines) = captured.lock() {
            for line in lines.iter() {
                println!("{line}");
            }
        }
        if let Some(error) = outcome.error {
            eprintln!("{error}");
        }
        return ExitCode::from(outcome.exit_code.clamp(0, 255) as u8);
    }
    let Some(script_index) = arguments
        .iter()
        .position(|arg| arg.ends_with(".js") || arg.ends_with(".mjs") || arg.ends_with(".cjs"))
    else {
        eprintln!("usage: cargo run -p quench-node-test --bin run -- <file.js>");
        return ExitCode::from(2);
    };
    let path = PathBuf::from(&arguments[script_index]);
    let exec_argv = arguments[..script_index].to_vec();
    let argv = arguments.into_iter().skip(script_index + 1).collect();
    let child_mode = std::env::var_os("QUENCH_CHILD_RUNNER").is_some();
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink_capture = Arc::clone(&captured);
    let sink: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line| {
        if let Ok(mut lines) = sink_capture.lock() {
            lines.push(line.to_string());
        }
    });
    let mut runner = NodeTestRunner::new().with_output_sink(sink);
    let outcome = runner.run_file_with_options(&path, argv, exec_argv);
    if child_mode {
        let lines = captured
            .lock()
            .map(|lines| lines.clone())
            .unwrap_or_default();
        for line in &lines {
            // The sink receives stream chunks, not logical lines. Preserve
            // their bytes when forwarding child stdout; adding another
            // newline changes Node's observable `spawnSync().stdout`.
            print!("{line}");
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
            if child_mode {
                let message = reason.strip_prefix("runtime: ").unwrap_or(&reason);
                // The fixture runner prefixes a rendered uncaught error with
                // its status for classification.  A real child process only
                // writes the rendered exception to stderr, so remove that
                // transport metadata before forwarding bytes to the parent.
                let (message, exit_code) = match message.strip_prefix("exit code ") {
                    Some(value) => {
                        let (code_text, detail) = value
                            .split_once(": ")
                            .map_or((value, ""), |(code, detail)| (code, detail));
                        let code = code_text.parse::<u8>().ok();
                        (detail, code)
                    }
                    None => (message, None),
                };
                if !message.is_empty() {
                    eprintln!("{message}");
                }
                return ExitCode::from(exit_code.unwrap_or(1));
            }
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

fn openssl_config_has_no_default_provider(arguments: &[String]) -> bool {
    let config = arguments.iter().enumerate().find_map(|(index, argument)| {
        argument
            .strip_prefix("--openssl-config=")
            .map(str::to_owned)
            .or_else(|| {
                (argument == "--openssl-config")
                    .then(|| arguments.get(index + 1).cloned())
                    .flatten()
            })
    });
    let Some(config) = config else { return false };
    let Ok(contents) = std::fs::read_to_string(config) else {
        return false;
    };
    let has_provider_section = contents
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case("[provider_sect]"));
    let has_default_provider = contents.lines().any(|line| {
        line.split_once('=').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("default")
                && value.trim().eq_ignore_ascii_case("default_sect")
        })
    });
    has_provider_section && !has_default_provider
}

fn node_option_disallowed(option: &str) -> bool {
    [
        "--version",
        "-v",
        "--help",
        "-h",
        "--eval",
        "-e",
        "--print",
        "-p",
        "-pe",
        "--check",
        "-c",
        "--interactive",
        "-i",
        "--v8-options",
        "--expose_internals",
        "--expose-internals",
        "--",
        "--test",
    ]
    .iter()
    .any(|&disallowed| option == disallowed || option.starts_with(&format!("{disallowed}=")))
}

fn node_exec_path() -> String {
    let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok())
    else {
        return "quench-node".into();
    };
    let launcher = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| matches!(stem, "run" | "run-compat" | "run-parallel"));
    if launcher {
        let engine = path.with_file_name("quench-node");
        if engine.is_file() {
            return engine.to_string_lossy().into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}
