#![cfg(not(test))]

use quench_node::run::{eval_script, eval_script_with_exec_argv, run_script_with_exec_argv};
use quench_runtime::vm::OutputSink;
use std::{
    env, fs,
    io::Write,
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
    if let Some(script_index) = args
        .iter()
        .position(|arg| arg.ends_with(".js") || arg.ends_with(".mjs") || arg.ends_with(".cjs"))
    {
        return run_file(
            Path::new(&args[script_index]),
            &args[script_index + 1..],
            &args[..script_index],
        );
    }
    let mode_index = args
        .iter()
        .enumerate()
        .position(|(index, arg)| {
            let trace_value = index > 0
                && matches!(
                    args[index - 1].as_str(),
                    "--trace-event-categories" | "--trace-event-file-pattern"
                );
            !trace_value
                && !arg.starts_with("--experimental-")
                && !arg.starts_with("--network-family-autoselection")
                && !arg.starts_with("--title=")
                && !arg.starts_with("--trace-event-categories")
                && !arg.starts_with("--trace-event-file-pattern")
                && !matches!(
                    arg.as_str(),
                    "--trace-events-enabled"
                        | "--no-warnings"
                        | "--expose-gc"
                        | "--expose-internals"
                        | "--expose_internal"
                )
        })
        .unwrap_or(args.len());
    match args.get(mode_index).map(String::as_str) {
        Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some("--version") | Some("-v") => {
            println!("v22.0.0");
            Ok(())
        }
        Some("-e") | Some("--eval") => {
            let source = args.get(mode_index + 1).map_or("", String::as_str);
            let sink: OutputSink = std::sync::Arc::new(|line| println!("{line}"));
            finish_outcome(eval_script_with_exec_argv(
                source,
                sink,
                false,
                &args[..mode_index],
            ))
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
        Some(path) => run_file(
            Path::new(path),
            &args[mode_index + 1..],
            &args[..mode_index],
        ),
        None => {
            let sink: OutputSink = std::sync::Arc::new(|line| println!("{line}"));
            match eval_script("", sink).error {
                Some(error) => Err(error.into()),
                None => Ok(()),
            }
        }
    }
}

/// Print the host's supported command-line surface.  This intentionally
/// describes quench-node's Rust boundary rather than pretending to be the
/// complete external Node executable.  Keep writes fallible: `node --help |
/// head` closes stdout early and a successful help invocation must still exit
/// with status zero.
fn print_help() {
    let color = colors_enabled();
    let (bold, green, magenta, blue, underline, reset) = if color {
        (
            "\x1b[1m", "\x1b[32m", "\x1b[35m", "\x1b[34m", "\x1b[4m", "\x1b[0m",
        )
    } else {
        ("", "", "", "", "", "")
    };
    let text = format!(
        "{bold}Usage:{reset} quench-node [options] [script.js] [arguments]\n\n\
{bold}Options:{reset}\n\
  {green}-e, --eval CODE{reset}       evaluate CODE\n\
  {green}-p, --print CODE{reset}      evaluate and print CODE\n\
  {green}-h, --help{reset}            print this help and exit\n\
  {green}-v, --version{reset}         print the runtime version and exit\n\
  {green}--openssl-config=...{reset}  load OpenSSL configuration\n\
  {green}--tls-cipher-list=...{reset} configure the TLS cipher list\n\
  {green}--use-bundled-ca{reset}      use the bundled certificate store\n\
  {green}--use-openssl-ca{reset}      use the OpenSSL certificate store\n\
  {green}--use-system-ca{reset}       use the system certificate store\n\
  {green}--enable-fips{reset}         enable FIPS mode\n\
  {green}--force-fips{reset}          force FIPS mode\n\
  {green}--icu-data-dir=...{reset}    set the ICU data directory\n\
  {magenta}NODE_ICU_DATA{reset}        override the ICU data directory\n\
For documentation, see {blue}{underline}https://nodejs.org/api/cli.html{reset}.\n"
    );
    let _ = std::io::stdout().write_all(text.as_bytes());
}

fn colors_enabled() -> bool {
    if env::var_os("NO_COLOR").is_some() || env::var_os("NODE_DISABLE_COLORS").is_some() {
        return false;
    }
    env::var_os("FORCE_COLOR").is_some_and(|value| value != "0")
}

fn run_file(
    path: &Path,
    script_args: &[String],
    exec_argv: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let sink: OutputSink = std::sync::Arc::new(|line| println!("{line}"));
    finish_outcome(run_script_with_exec_argv(
        path,
        script_args,
        exec_argv,
        &source,
        sink,
    ))
}

fn finish_outcome(outcome: quench_node::run::RunOutcome) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(error) = outcome.error {
        eprintln!("{error}");
    }
    if outcome.exit_code != 0 {
        process::exit(outcome.exit_code.clamp(0, 255) as i32);
    }
    Ok(())
}

fn run_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if dir.is_file() {
        return run_file(dir, &[], &[]);
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
            match run_file(entry.path(), &[], &[]) {
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
