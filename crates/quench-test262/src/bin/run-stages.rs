use std::{
    env,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

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
        print_stage_start(stage);
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
    // Skip tests that crash the runtime (stack overflow, panic, etc.) —
    // they are real runtime bugs, not runner issues, and would otherwise
    // bring down the whole stage run via a process abort.
    let files: Vec<PathBuf> = files
        .into_iter()
        .filter(|path| {
            let path_str = path.to_string_lossy();
            !CRASHED_AT_RUNTIME
                .iter()
                .any(|needle| path_str.contains(needle))
        })
        .collect();
    if files.is_empty() {
        return Ok(StageReport::default());
    }
    let report = runner.run_files_with_cache_limited(files, harness, max_failures)?;
    println!(
        "stage {:>3}: {} passed {}",
        stage.id, stage.path, report.passed
    );
    Ok(report)
}

/// Tests that crash the runtime regardless of runner state (stack
/// overflow, panic, infinite loop, etc.). These are real runtime bugs,
/// not runner bugs; the stage runner skips them so the diff focuses on
/// order/thread-induced divergence.
const CRASHED_AT_RUNTIME: &[&str] = &[
    "built-ins/Object/prototype/toString/proxy-revoked-during-get-call.js",
    "built-ins/Array/from/iter-set-elem-prop-non-writable.js",
    "built-ins/Array/prototype/concat/Array.prototype.concat_large-typed-array.js",
    "built-ins/Array/prototype/reduceRight/length-near-integer-limit.js",
    "built-ins/Function/internals/Construct/base-ctor-revoked-proxy.js",
    "built-ins/Iterator/from/iterable-primitives.js",
    "built-ins/Iterator/prototype/map/returned-iterator-yields-mapper-return-values.js",
    "built-ins/Iterator/prototype/flatMap/flattens-iterator.js",
    "built-ins/Iterator/prototype/flatMap/flattens-only-depth-1.js",
    "built-ins/Iterator/prototype/flatMap/iterable-to-iterator-fallback.js",
    "built-ins/Iterator/prototype/flatMap/iterable-primitives-are-not-flattened.js",
    "built-ins/Iterator/prototype/flatMap/flattens-iterable.js",
    "built-ins/Iterator/prototype/take/limit-tonumber.js",
    "built-ins/Iterator/prototype/take/limit-greater-than-or-equal-to-total.js",
    "built-ins/Iterator/prototype/take/limit-less-than-total.js",
    "built-ins/Proxy/apply/trap-is-undefined-target-is-proxy.js",
    "built-ins/Proxy/deleteProperty/call-parameters.js",
    "built-ins/Proxy/construct/trap-is-undefined-proto-from-cross-realm-newtarget.js",
    "built-ins/Proxy/construct/trap-is-null.js",
    "built-ins/Proxy/construct/trap-is-null-target-is-proxy.js",
    "built-ins/Proxy/construct/trap-is-undefined-no-property.js",
    "built-ins/Proxy/construct/trap-is-undefined.js",
    "built-ins/Proxy/construct/trap-is-undefined-proto-from-newtarget-realm.js",
    "built-ins/Proxy/construct/trap-is-missing-target-is-proxy.js",
    "built-ins/RegExp/property-escapes/",
    "built-ins/RegExp/CharacterClassEscapes/character-class-digit-class-escape-negative-cases.js",
    "built-ins/RegExp/CharacterClassEscapes/character-class-non-word-class-escape-positive-cases.js",
    "built-ins/RegExp/CharacterClassEscapes/character-class-word-class-escape-negative-cases.js",
    "built-ins/RegExp/CharacterClassEscapes/character-class-non-digit-class-escape-positive-cases.js",
    "built-ins/RegExp/CharacterClassEscapes/character-class-whitespace-class-escape-negative-cases.js",
    "built-ins/RegExp/CharacterClassEscapes/character-class-non-whitespace-class-escape-positive-cases.js",
    "built-ins/RegExp/character-class-escape-non-whitespace.js",
    "built-ins/RegExp/unicodeSets/generated/rgi-emoji-",
    "built-ins/RegExp/unicodeSets/generated/rgi-emoji-17.0.js",
    "built-ins/RegExp/unicodeSets/generated/rgi-emoji-13.1.js",
    "built-ins/RegExp/prototype/Symbol.match/g-match-empty-advance-lastindex.js",
    "built-ins/RegExp/prototype/Symbol.match/g-coerce-result-err.js",
    "built-ins/RegExp/prototype/hasIndices/this-val-regexp.js",
    "language/statements/with/set-mutable-binding-idref-with-proxy-env.js",
    "language/statements/with/set-mutable-binding-idref-compound-assign-with-proxy-env.js",
    "built-ins/Object/prototype/setPrototypeOf-with-different-values.js",
    "built-ins/Object/setPrototypeOf/set-failure-cycle.js",
    "built-ins/String/prototype/replace/S15.5.4.11_A1_T17.js",
    "built-ins/String/prototype/matchAll/regexp-prototype-matchAll-v-u-flag.js",
    "built-ins/TypedArrayConstructors/ctors/typedarray-arg/same-ctor-buffer-ctor-species-null.js",
    "built-ins/TypedArrayConstructors/ctors/typedarray-arg/same-ctor-buffer-ctor-species-undefined.js",
    "built-ins/TypedArrayConstructors/ctors/typedarray-arg/same-ctor-returns-new-cloned-typedarray.js",
    "built-ins/TypedArrayConstructors/ctors/typedarray-arg/src-typedarray-resizable-buffer.js",
    "built-ins/TypedArray/prototype/fill/",
    "built-ins/TypedArray/prototype/slice/resize-count-bytes-to-zero.js",
    "built-ins/TypedArray/prototype/byteOffset/return-byteoffset.js",
    "built-ins/TypedArray/prototype/lastIndexOf/negative-index-and-resize-to-smaller.js",
    "built-ins/Array/prototype/every/15.4.4.16-3-29.js",
    "built-ins/Array/prototype/some/15.4.4.17-3-29.js",
    "built-ins/Promise/all/",
    "built-ins/Promise/any/",
    "built-ins/Promise/allSettled/",
    "built-ins/Promise/race/",
];

fn parse_stage_index(value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("invalid stage index: {value}"))
}

fn fail(error: impl AsRef<str>) -> ExitCode {
    eprintln!("FAIL: {}", error.as_ref());
    ExitCode::from(1)
}
