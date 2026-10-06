//! Run the checked-in Node parallel profile through the shared VM.

use quench_node_test::{case_process::worker_entry_with, parallel_profile, shared_runner};
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if let Some(code) = worker_entry_with(&arguments, shared_runner::run_parallel_fixture) {
        return code;
    }
    let options = match Options::parse(&arguments) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    };
    if options.help {
        println!("run-parallel-next --profile NAME [--filter NAME] [--timeout-secs N]");
        return ExitCode::SUCCESS;
    }
    parallel_profile::run(
        Some(&options.profile),
        options.filter.as_deref(),
        options.timeout_secs,
    )
}

struct Options {
    profile: String,
    filter: Option<String>,
    timeout_secs: u64,
    help: bool,
}

impl Options {
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut profile = None;
        let mut filter = None;
        let mut timeout_secs = quench_node_test::case_process::DEFAULT_CASE_TIMEOUT_SECS;
        let mut help = false;
        let mut args = arguments.iter();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--help" | "-h" => help = true,
                "--profile" => profile = Some(next_value(&mut args, "--profile")?),
                "--filter" => filter = Some(next_value(&mut args, "--filter")?),
                "--timeout-secs" => {
                    timeout_secs = next_value(&mut args, "--timeout-secs")?
                        .parse()
                        .map_err(|_| "--timeout-secs must be a positive integer")?;
                    if timeout_secs == 0 {
                        return Err("--timeout-secs must be a positive integer".into());
                    }
                }
                value => return Err(format!("unknown option {value}")),
            }
        }
        let profile = if help {
            profile.unwrap_or_default()
        } else {
            profile.ok_or("--profile is required")?
        };
        Ok(Self {
            profile,
            filter,
            timeout_secs,
            help,
        })
    }
}

fn next_value<'a>(
    args: &mut impl Iterator<Item = &'a String>,
    option: &str,
) -> Result<String, String> {
    args.next()
        .cloned()
        .ok_or_else(|| format!("{option} requires a value"))
}
