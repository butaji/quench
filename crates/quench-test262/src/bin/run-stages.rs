use std::{env, path::Path, path::PathBuf, process::ExitCode};

use quench_test262::{
    discover_js_files, resolve_stages, HarnessCache, ResolvedStage, RuntimeHost, StageReport,
    Test262Runner,
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

fn main() -> ExitCode {
    const STACK_SIZE: usize = 256 * 1024 * 1024;
    let handle = match std::thread::Builder::new()
        .name("run-stages-main".to_string())
        .stack_size(STACK_SIZE)
        .spawn(run_stages_entry)
    {
        Ok(handle) => handle,
        Err(error) => return fail(error.to_string()),
    };
    match handle.join() {
        Ok(code) => code,
        Err(_) => ExitCode::from(1),
    }
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
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut harness = HarnessCache::new(root.join("harness"));
    let mut overall = StageReport::default();
    let mut has_failure = false;
    for stage in &stages {
        let report = match run_single_stage(&mut runner, &mut harness, stage, args.max_failures) {
            Ok(report) => report,
            Err(error) => return fail(error),
        };
        if report.failed > 0 {
            has_failure = true;
            print_stage_failures(stage, &report, args);
            if !args.continue_on_failure {
                return ExitCode::from(1);
            }
        }
        accumulate_stage_report(&mut overall, &report);
    }
    if has_failure {
        return ExitCode::from(1);
    }
    print_stage_totals(stages.len(), overall);
    ExitCode::SUCCESS
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

fn run_single_stage<H: quench_test262::Test262Host>(
    runner: &mut Test262Runner<H>,
    harness: &mut HarnessCache,
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
    let report = runner.run_files_with_cache_limited(files, harness, max_failures)?;
    println!(
        "stage {:>3}: {} passed {}",
        stage.id, stage.path, report.passed
    );
    Ok(report)
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
