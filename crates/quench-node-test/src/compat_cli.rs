//! Shared selection, inventory validation and reporting for Node compatibility runners.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::case_process::{RunResult, DEFAULT_CASE_TIMEOUT_SECS};
use crate::stages::discover_fixtures;

pub fn run(arguments: impl IntoIterator<Item = String>) -> ExitCode {
    let arguments: Vec<_> = arguments.into_iter().collect();
    let command = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "run-compat".into());
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("{command}: run Node compatibility cases through the selected development entry");
        println!();
        println!(
            "usage: {command} [--list] [--filter NAME] [--subset] [--quiet] [--trace-observations] [DIR | --inventory PATH]"
        );
        println!("  --list           enumerate the suite instead of running it");
        println!("  --filter NAME    only run scripts whose name contains NAME");
        println!("  --quiet          skip per-test output, only the summary");
        println!("  --inventory PATH use reviewed case membership; unfrozen lists are diagnostic");
        println!(
            "  --subset         run a filtered inventory subset as diagnostic, never as a gate"
        );
        println!(
            "  --trace-observations capture matched Node/shared-VM traces only; does not qualify membership"
        );
        println!(
            "  DIR              compat suite root (default: crates/quench-node-test/node-tests)"
        );
        return ExitCode::SUCCESS;
    }
    let options = match Options::parse(arguments.into_iter()) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    };
    run_with_options(options)
}

struct Options {
    dir: PathBuf,
    list: bool,
    quiet: bool,
    filter: Option<String>,
    inventory: Option<PathBuf>,
    subset: bool,
    trace_observations: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            dir: PathBuf::from("crates/quench-node-test/node-tests"),
            list: false,
            quiet: false,
            filter: None,
            inventory: None,
            subset: false,
            trace_observations: false,
        };
        let mut positional = None;
        let mut args = args;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--list" => options.list = true,
                "--quiet" => options.quiet = true,
                "--subset" => options.subset = true,
                "--trace-observations" => options.trace_observations = true,
                "--filter" => {
                    options.filter = Some(args.next().ok_or("--filter requires a name")?);
                }
                "--inventory" => {
                    if options.inventory.is_some() {
                        return Err("only one inventory may be specified".into());
                    }
                    options.inventory = Some(PathBuf::from(
                        args.next().ok_or("--inventory requires a path")?,
                    ));
                }
                value if value.starts_with('-') => return Err(format!("unknown option {value}")),
                value if positional.is_none() => positional = Some(PathBuf::from(value)),
                _ => return Err("only one directory may be specified".into()),
            }
        }
        let has_positional = positional.is_some();
        if let Some(dir) = positional {
            if options.inventory.is_some() {
                return Err("a directory and --inventory cannot be combined".into());
            }
            options.dir = dir;
        }
        if options.subset && (options.inventory.is_none() || options.filter.is_none()) {
            return Err("--subset requires --inventory and --filter".into());
        }
        if options.subset && options.list {
            return Err("--subset applies only to execution, not listing".into());
        }
        if options.trace_observations
            && (options.inventory.is_none()
                || options.filter.is_some()
                || options.subset
                || options.list
                || has_positional)
        {
            return Err(
                "--trace-observations requires --inventory and cannot be combined with filters or a directory".into(),
            );
        }
        Ok(options)
    }
}

fn run_with_options(options: Options) -> ExitCode {
    let repository = match std::env::current_dir() {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("error: repository directory: {error}");
            return ExitCode::from(2);
        }
    };
    let inventory = match options
        .inventory
        .as_deref()
        .map(|path| crate::inventory::NodeInventory::read(path, &repository))
        .transpose()
    {
        Ok(inventory) => inventory,
        Err(error) => {
            eprintln!("error: Node inventory: {error}");
            return ExitCode::from(2);
        }
    };
    if options.trace_observations {
        let Some(inventory) = inventory.as_ref() else {
            eprintln!("error: --trace-observations requires --inventory");
            return ExitCode::from(2);
        };
        return match crate::node_observations::trace_observations(inventory, &repository) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("error: Node observation traces: {error}");
                ExitCode::from(2)
            }
        };
    }
    let fixtures = match inventory.as_ref() {
        Some(inventory) => Ok(inventory.included_paths(&repository)),
        None => discover_fixtures(&options.dir).map_err(|error| error.to_string()),
    };
    let fixtures = match fixtures {
        Ok(fixtures) => fixtures,
        Err(error) => {
            eprintln!("error: fixture discovery: {error}");
            return ExitCode::from(2);
        }
    };
    if fixtures.is_empty() {
        eprintln!("error: no executable cases selected");
        return ExitCode::from(2);
    }
    let fixtures = filter_fixtures(fixtures, options.filter.as_deref());
    if fixtures.is_empty() {
        if let Some(filter) = options.filter.as_deref() {
            eprintln!("error: no fixtures match --filter {filter:?}");
        } else {
            eprintln!("error: no fixtures remain after discovery");
        }
        return ExitCode::from(2);
    }
    if options.list {
        if inventory.is_some() {
            eprintln!(
                "inventory listing is diagnostic; it does not qualify implemented membership or case outcomes"
            );
        }
        for f in &fixtures {
            if inventory.is_some() {
                println!("{}", f.strip_prefix(&repository).unwrap_or(f).display());
            } else {
                println!("{}", f.file_name().unwrap().to_string_lossy());
            }
        }
        return ExitCode::SUCCESS;
    }
    if let Some(inventory) = &inventory {
        let validation = if options.subset {
            inventory.validate_subset_execution(&repository, &fixtures)
        } else {
            inventory.validate_execution(&repository, &fixtures)
        };
        if let Err(error) = validation {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    }
    let summary = match run_suite(&fixtures, options.quiet, &repository) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    print_summary(&summary, fixtures.len(), options.subset);
    if summary.passed == fixtures.len() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn filter_fixtures(
    fixtures: Vec<std::path::PathBuf>,
    filter: Option<&str>,
) -> Vec<std::path::PathBuf> {
    match filter {
        Some(name) => fixtures
            .into_iter()
            .filter(|f| f.file_name().unwrap().to_string_lossy().contains(name))
            .collect(),
        None => fixtures,
    }
}

struct SuiteSummary {
    passed: usize,
    failed: usize,
    skipped: usize,
    failed_names: Vec<String>,
}

fn run_suite(
    fixtures: &[PathBuf],
    quiet: bool,
    repository: &std::path::Path,
) -> Result<SuiteSummary, String> {
    use std::io::Write;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut summary = SuiteSummary {
        passed: 0,
        failed: 0,
        skipped: 0,
        failed_names: Vec::new(),
    };
    for fixture in fixtures {
        let observation = crate::case_process::observe_inventory_case(
            &executable,
            fixture,
            repository,
            std::time::Duration::from_secs(DEFAULT_CASE_TIMEOUT_SECS),
        )?;
        std::io::stdout()
            .write_all(&observation.stdout)
            .map_err(|error| error.to_string())?;
        std::io::stderr()
            .write_all(&observation.stderr)
            .map_err(|error| error.to_string())?;
        let outcome = observation.outcome();
        if !quiet {
            match &observation.worker {
                Some(crate::NodeOutcome::Fail { reason } | crate::NodeOutcome::Skip { reason })
                    if matches!(outcome, RunResult::Fail | RunResult::Skip) =>
                {
                    println!(
                        "{}  {}: {reason}",
                        outcome.label().to_uppercase(),
                        fixture.display()
                    );
                }
                Some(crate::NodeOutcome::GuestExit { code })
                    if outcome == RunResult::Unclassified =>
                {
                    println!(
                        "UNCLASSIFIED  {}: guest exit status {code} requires harness classification",
                        fixture.display()
                    );
                }
                _ => println!("{}  {}", outcome.label().to_uppercase(), fixture.display()),
            }
        }
        match outcome {
            RunResult::Pass => summary.passed += 1,
            RunResult::Skip => summary.skipped += 1,
            _ => {
                summary.failed += 1;
                summary.failed_names.push(fixture.display().to_string());
            }
        }
    }
    Ok(summary)
}

fn print_summary(summary: &SuiteSummary, total: usize, subset: bool) {
    let label = if subset {
        "compat subset (diagnostic)"
    } else {
        "compat"
    };
    println!(
        "\n{label}: {passed} passed, {failed} failed, {skipped} skipped, {total} total",
        label = label,
        passed = summary.passed,
        failed = summary.failed,
        skipped = summary.skipped,
    );
    if !summary.failed_names.is_empty() {
        println!("failures:");
        for name in &summary.failed_names {
            println!("  - {name}");
        }
    }
}
