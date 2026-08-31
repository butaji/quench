use std::{
    env,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
};

use quench_test262::{
    discover_js_files, resolve_stages, HarnessCache, ResolvedStage, RuntimeHost, StageReport,
    Test262Runner, TestOutcome,
};

#[derive(Debug)]
struct Args {
    from: u32,
    to: Option<u32>,
    continue_on_failure: bool,
    max_failures: usize,
    list: bool,
    root: PathBuf,
}

type StageFileResult = (usize, PathBuf, Result<TestOutcome, String>);

fn main() -> ExitCode {
    run_stages_entry()
}

fn run_stages_entry() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => return fail(error),
    };
    let stages = match resolve_stages(&args.root) {
        Ok(stages) => stages,
        Err(error) => return fail(error),
    };
    if args.list {
        list_stages(&stages);
        return ExitCode::SUCCESS;
    }
    let selected = match select_stages(&stages, &args) {
        Ok(selected) => selected,
        Err(error) => return fail(error),
    };
    run_stages(&args.root, selected, &args)
}

fn select_stages(stages: &[ResolvedStage], args: &Args) -> Result<Vec<ResolvedStage>, String> {
    let to_id = args
        .to
        .unwrap_or(stages.last().map(|stage| stage.id).unwrap_or_default());
    if args.from > to_id {
        return Err(format!(
            "invalid stage range {}..={} (from greater than to)",
            args.from, to_id
        ));
    }
    let start = find_stage_index(stages, args.from)
        .ok_or_else(|| format!("stage {} not found in docs/STAGES.md", args.from))?;
    let end = if let Some(to_stage_id) = args.to {
        let end = find_stage_index(stages, to_stage_id)
            .ok_or_else(|| format!("stage {} not found in docs/STAGES.md", to_stage_id))?;
        if end < start {
            return Err(format!(
                "invalid stage range {}..={} (from greater than to)",
                args.from, to_id
            ));
        }
        end
    } else {
        stages.len() - 1
    };
    let selected = stages[start..=end].to_vec();
    if selected.is_empty() {
        return Err(format!("no stages in range {}..={}", args.from, to_id));
    }
    Ok(selected)
}

fn parse_args() -> Result<Args, String> {
    let mut raw = parse_cli_tokens(env::args().skip(1).collect::<Vec<_>>())?;
    if raw.positionals.len() > 1 {
        return Err(
            "usage: run-stages [--from N] [--to N] [--continue] [--max-failures N] [--list] [--root <dir>] [N]"
                .to_string(),
        );
    }
    if let Some(end) = raw.positionals.into_iter().next() {
        raw.to = Some(parse_stage_index(&end)?);
    }
    Ok(Args {
        from: raw.from,
        to: raw.to,
        continue_on_failure: raw.continue_on_failure,
        max_failures: raw.max_failures,
        list: raw.list,
        root: raw.root.unwrap_or_else(|| PathBuf::from("tests/test262")),
    })
}

struct RawArgs {
    from: u32,
    to: Option<u32>,
    continue_on_failure: bool,
    max_failures: usize,
    list: bool,
    root: Option<PathBuf>,
    positionals: Vec<String>,
}

fn parse_cli_tokens(args: Vec<String>) -> Result<RawArgs, String> {
    let mut parser = CliParser {
        args,
        index: 0,
        from: 0,
        to: None,
        continue_on_failure: false,
        max_failures: 12,
        list: false,
        root: env::var_os("TEST262_DIR").map(PathBuf::from),
        positionals: Vec::new(),
    };
    while parser.index < parser.args.len() {
        parse_cli_token(&mut parser)?;
        parser.index += 1;
    }
    Ok(RawArgs {
        from: parser.from,
        to: parser.to,
        continue_on_failure: parser.continue_on_failure,
        max_failures: parser.max_failures,
        list: parser.list,
        root: parser.root,
        positionals: parser.positionals,
    })
}

struct CliParser {
    args: Vec<String>,
    index: usize,
    from: u32,
    to: Option<u32>,
    continue_on_failure: bool,
    max_failures: usize,
    list: bool,
    root: Option<PathBuf>,
    positionals: Vec<String>,
}

impl CliParser {
    fn token(&self) -> Result<&str, String> {
        self.args
            .get(self.index)
            .map(String::as_str)
            .ok_or_else(|| "empty cli args".to_string())
    }

    fn parse_stage(&mut self, flag: &str) -> Result<u32, String> {
        self.advance(flag)?;
        parse_stage_index(self.value(flag)?)
    }

    fn parse_usize(&mut self, flag: &str) -> Result<usize, String> {
        self.advance(flag)?;
        self.value(flag)?
            .parse::<usize>()
            .map_err(|_| "invalid --max-failures value".to_string())
    }

    fn parse_root(&mut self, flag: &str) -> Result<PathBuf, String> {
        self.advance(flag)?;
        Ok(PathBuf::from(self.value(flag)?))
    }

    fn value(&self, flag: &str) -> Result<&str, String> {
        self.args
            .get(self.index)
            .map(String::as_str)
            .ok_or_else(|| format!("{flag} requires an argument"))
    }

    fn advance(&mut self, flag: &str) -> Result<(), String> {
        self.index += 1;
        if self.index >= self.args.len() {
            return Err(format!("{flag} requires an argument"));
        }
        Ok(())
    }
}

fn parse_cli_token(parser: &mut CliParser) -> Result<(), String> {
    match parser.token()? {
        "--from" => parser.from = parser.parse_stage("--from")?,
        "--to" => parser.to = Some(parser.parse_stage("--to")?),
        "--continue" => parser.continue_on_failure = true,
        "--max-failures" => parser.max_failures = parser.parse_usize("--max-failures")?,
        "--list" => parser.list = true,
        "--root" => parser.root = Some(parser.parse_root("--root")?),
        arg if arg.starts_with("--") => return Err(format!("unknown option: {arg}")),
        arg => parser.positionals.push(arg.to_string()),
    }
    Ok(())
}

fn run_stages(root: &Path, stages: Vec<ResolvedStage>, args: &Args) -> ExitCode {
    let mut overall = StageReport::default();
    let mut has_failure = false;
    for stage in &stages {
        print_stage_start(stage);
        let report = match run_single_stage(root, stage, args.max_failures) {
            Ok(report) => report,
            Err(error) => return fail(error),
        };
        accumulate_stage_report(&mut overall, &report);
        if report.failed > 0 {
            has_failure = true;
            print_stage_failures(stage, &report, args);
            if !args.continue_on_failure {
                print_stage_totals(stages.len(), overall);
                return ExitCode::from(1);
            }
        }
    }
    print_stage_totals(stages.len(), overall);
    if has_failure {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn print_stage_start(stage: &ResolvedStage) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "stage {:>3}: {} starting", stage.id, stage.path);
    let _ = stdout.flush();
}

fn accumulate_stage_report(overall: &mut StageReport, report: &StageReport) {
    overall.total += report.total;
    overall.passed += report.passed;
    overall.failed += report.failed;
}

fn print_stage_totals(stages: usize, overall: StageReport) {
    println!(
        "stages={} passed={} failed={} total={}",
        stages, overall.passed, overall.failed, overall.total
    );
}

fn print_stage_failures(stage: &ResolvedStage, report: &StageReport, args: &Args) {
    println!(
        "stage {:>3}: {} failed {}/{}",
        stage.id, stage.path, report.failed, report.total
    );
    for (path, reason) in report.failures.iter().take(args.max_failures) {
        println!("  {}: {}", path.display(), reason);
    }
    if report.failed > args.max_failures {
        println!(
            "  ... plus {} more failures",
            report.failed - args.max_failures
        );
    }
}

fn find_stage_index(stages: &[ResolvedStage], id: u32) -> Option<usize> {
    stages.iter().position(|stage| stage.id == id)
}

fn list_stages(stages: &[ResolvedStage]) {
    for stage in stages {
        println!("{:>3}: {}", stage.id, stage.path);
    }
}

fn run_single_stage(
    root: &Path,
    stage: &ResolvedStage,
    max_failures: usize,
) -> Result<StageReport, String> {
    let files = discover_js_files(&stage.root)?;
    if files.is_empty() {
        println!(
            "stage {:>3}: {} ({}): no files",
            stage.id,
            stage.path,
            stage.root.display()
        );
        return Ok(StageReport::default());
    }
    let isolated = stage_process_isolation(files.len());
    let results = run_stage_files(root, files, isolated)?;
    let mut report = StageReport::default();
    for (_, path, outcome) in results {
        report.total += 1;
        match outcome? {
            TestOutcome::Pass => report.passed += 1,
            TestOutcome::Fail { reason } => {
                report.failed += 1;
                if report.failures.len() < max_failures {
                    report.failures.push((path, reason));
                }
            }
        }
    }
    println!(
        "stage {:>3}: {} passed {}",
        stage.id, stage.path, report.passed
    );
    Ok(report)
}

fn run_stage_files(
    root: &Path,
    files: Vec<PathBuf>,
    isolated: bool,
) -> Result<Vec<StageFileResult>, String> {
    // Child processes bound allocator retention. Four-fixture batches keep
    // process startup amortized while remaining disposable for pathological
    // patterns; in-process stages use the same modest batch.
    let work_batch = 4;
    // OXC and the reducer need more than the platform default for deeply
    // nested fixtures, but 256 MiB per worker makes a four-worker RegExp
    // stage reserve a gigabyte before the engine heap is counted.
    const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;
    let worker_count = env::var("QUENCH_STAGE_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| *count > 0)
        .unwrap_or(4)
        .min(files.len());
    let files = Arc::new(files);
    let next = Arc::new(AtomicUsize::new(0));
    let (sender, receiver) = mpsc::sync_channel(worker_count * work_batch);
    let mut workers = Vec::with_capacity(worker_count);
    for worker in 0..worker_count {
        let files = Arc::clone(&files);
        let next = Arc::clone(&next);
        let root = root.to_path_buf();
        let sender = sender.clone();
        workers.push(
            std::thread::Builder::new()
                .name(format!("stage-files-{worker}"))
                .stack_size(WORKER_STACK_SIZE)
                .spawn(move || {
                    let mut runner = Test262Runner::new(RuntimeHost);
                    let mut harness = HarnessCache::new(root.join("harness"));
                    loop {
                        let start = next.fetch_add(work_batch, Ordering::Relaxed);
                        if start >= files.len() {
                            break;
                        }
                        let stop = (start + work_batch).min(files.len());
                        if isolated {
                            let outcomes = run_files_in_process(&root, &files[start..stop]);
                            for (offset, outcome) in batch_outcomes(outcomes, stop - start) {
                                if sender.send((start + offset, outcome)).is_err() {
                                    return;
                                }
                            }
                        } else {
                            for index in start..stop {
                                let path = &files[index];
                                let outcome = runner.run_file_with_cache(path, &mut harness);
                                if sender.send((index, outcome)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                })
                .map_err(|error| format!("stage worker spawn failed: {error}"))?,
        );
    }
    drop(sender);
    let mut results: Vec<_> = receiver.into_iter().collect();
    for worker in workers {
        worker
            .join()
            .map_err(|_| "stage worker panicked".to_string())?;
    }
    results.sort_by_key(|(index, _)| *index);
    let mut files = Arc::try_unwrap(files)
        .map_err(|_| "stage files still referenced after workers joined".to_string())?;
    Ok(results
        .into_iter()
        .map(|(index, outcome)| (index, std::mem::take(&mut files[index]), outcome))
        .collect())
}

/// Long in-process sweeps retain unreachable `Rc` cycles until the worker
/// exits. Recycle bounded fixture batches in child processes for large stages
/// so the runner's memory bound is independent of fixture graph shape.
fn stage_process_isolation(file_count: usize) -> bool {
    const MAX_IN_PROCESS_FILES: usize = 64;
    env::var_os("QUENCH_STAGE_PROCESS_ISOLATION").is_some()
        || (file_count > MAX_IN_PROCESS_FILES && run_test_executable().is_some())
}

fn run_test_executable() -> Option<std::path::PathBuf> {
    env::current_exe()
        .ok()
        .map(|executable| executable.with_file_name("run-test"))
        .filter(|executable| executable.is_file())
}

fn run_files_in_process(root: &Path, paths: &[PathBuf]) -> Result<Vec<TestOutcome>, String> {
    let executable = run_test_executable()
        .ok_or_else(|| "stage run-test executable is unavailable".to_string())?;
    let output = Command::new(executable)
        .env("TEST262_DIR", root)
        .arg("--batch")
        .args(paths)
        .stdout(std::process::Stdio::piped())
        .output()
        .map_err(|error| format!("stage test process failed: {error}"))?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if reason.is_empty() {
            format!("test process exited with {}", output.status)
        } else {
            reason
        });
    }
    let encoded = String::from_utf8_lossy(&output.stdout);
    let reasons = serde_json::from_str::<Vec<Option<String>>>(&encoded)
        .map_err(|error| format!("invalid batch result: {error}"))?;
    if reasons.len() != paths.len() {
        return Err(format!(
            "batch result count {} does not match path count {}",
            reasons.len(),
            paths.len()
        ));
    }
    Ok(reasons
        .into_iter()
        .map(|reason| match reason {
            Some(reason) => TestOutcome::Fail { reason },
            None => TestOutcome::Pass,
        })
        .collect())
}

fn batch_outcomes(
    outcomes: Result<Vec<TestOutcome>, String>,
    count: usize,
) -> Vec<(usize, Result<TestOutcome, String>)> {
    match outcomes {
        Ok(outcomes) => outcomes
            .into_iter()
            .enumerate()
            .map(|(offset, outcome)| (offset, Ok(outcome)))
            .collect(),
        Err(error) => (0..count)
            .map(|offset| (offset, Err(error.clone())))
            .collect(),
    }
}

fn parse_stage_index(value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("invalid stage index: {value}"))
}

fn fail(error: impl AsRef<str>) -> ExitCode {
    eprintln!("FAIL: {}", error.as_ref());
    ExitCode::from(1)
}
