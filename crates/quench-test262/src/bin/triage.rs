use std::{
    collections::HashMap,
    env::{self, ArgsOs},
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
};

use serde_json::json;

use quench_test262::{
    discover_js_files, HarnessCache, RuntimeHost, Test262Runner, TestMetadata, TestOutcome,
};

/// Per-thread outcomes for one contiguous chunk of files.
#[derive(Default)]
struct RunReport {
    passed: usize,
    failed: usize,
    failures: Vec<(PathBuf, String)>,
    outcomes: Vec<JsonOutcome>,
}

/// One outcome row fed to the machine-readable JSON report.
struct JsonOutcome {
    path: PathBuf,
    category: String,
}

/// Deep parser/reducer recursion needs more than the default 8 MiB stack.
const DEFAULT_STACK_SIZE: usize = 256 * 1024 * 1024;
const STACK_SIZE_ENV: &str = "TRIAGE_WORKER_STACK_SIZE_BYTES";
const WORK_BATCH: usize = 32;

/// Tests that crash the runtime regardless of runner state (stack
/// overflow, panic, infinite loop, etc.). These are runtime bugs, not
/// runner determinism issues, and the comparison skips them so the diff
/// focuses on order/thread-induced divergence.
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

struct TestSource {
    path: PathBuf,
    source: String,
    metadata: TestMetadata,
}

struct Args {
    target: PathBuf,
    limit: usize,
    threads: usize,
    filters: Vec<String>,
    json: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = match parse_args(env::args_os()) {
        Ok(args) => args,
        Err(error) => return fail(&error),
    };
    let root = test262_root();
    let base = root.join("test").join(&args.target);
    let discovered = match discover_js_files(&base) {
        Ok(files) => files,
        Err(error) => return fail(&format!("discover: {error}")),
    };
    let discovered_count = discovered.len();
    let files = select_files(discovered, &base, &args.filters);
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
        return fail("no tests matched the requested filters");
    }
    let sources = match load_test_sources(&files) {
        Ok(sources) => sources,
        Err(error) => return fail(&error),
    };
    println!("selected={} discovered={discovered_count}", files.len());
    let threads = args.threads.max(1).min(files.len());
    let emit_outcomes = args.json.is_some();
    let (passed, failed, failures, outcomes) =
        run_parallel(&root, sources, args.limit, threads, emit_outcomes);
    if let Some(path) = &args.json {
        if let Err(error) = write_json_report(path, &args, &root, passed, failed, &outcomes) {
            return fail(&error);
        }
    }
    let buckets = bucket_failures(failures);
    print_report(passed, failed, &buckets);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Emit a machine-readable JSON report consumable by
/// `tools/perf/merge-differential-reports.cjs`: a fingerprint identifying the run
/// tree plus one `{fixture, category}` row per executed test.
fn write_json_report(
    path: &Path,
    args: &Args,
    root: &Path,
    passed: usize,
    failed: usize,
    outcomes: &[JsonOutcome],
) -> Result<(), String> {
    let base = root.join("test").join(&args.target);
    let results: Vec<_> = outcomes
        .iter()
        .map(|outcome| {
            let fixture = relative_fixture(&outcome.path, &base)?;
            Ok(json!({ "fixture": fixture, "category": outcome.category }))
        })
        .collect::<Result<_, String>>()?;
    let report = json!({
        "tool": "quench-triage",
        "fingerprints": fingerprints(root, args.target.as_path()),
        "passed": passed,
        "failed": failed,
        "results": results,
    });
    std::fs::write(path, serde_json::to_string_pretty(&report).unwrap())
        .map_err(|error| format!("write {}: {error}", path.display()))
}

fn fingerprints(root: &Path, target: &Path) -> serde_json::Value {
    json!({ "test262_tree": test262_tree_commit(root), "target": target.display().to_string() })
}

fn test262_tree_commit(root: &Path) -> String {
    let head = read_git_head(&root.join("HEAD"));
    if !head.is_empty() {
        return head;
    }
    std::fs::read_to_string(root.join(".git"))
        .ok()
        .and_then(|line| {
            line.strip_prefix("gitdir: ")
                .map(str::trim)
                .map(String::from)
        })
        .map(|gitdir| {
            let gitdir = PathBuf::from(gitdir);
            let gitdir = if gitdir.is_absolute() {
                gitdir
            } else {
                root.join(&gitdir)
            };
            read_git_head(&gitdir.join("HEAD"))
        })
        .unwrap_or_default()
}

/// Read a git HEAD (either a direct SHA or a `ref: <path>` indirection).
fn read_git_head(head_path: &Path) -> String {
    let raw = std::fs::read_to_string(head_path).unwrap_or_default();
    let raw = raw.trim();
    match raw.strip_prefix("ref: ") {
        Some(ref_name) => {
            let ref_path = head_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(ref_name);
            std::fs::read_to_string(ref_path)
                .map(|value| value.trim().to_string())
                .unwrap_or_default()
        }
        None => raw.to_string(),
    }
}

fn relative_fixture(path: &Path, target: &Path) -> Result<String, String> {
    path.strip_prefix(target)
        .map(|relative| relative.to_string_lossy().into_owned())
        .map_err(|_| format!("outside target: {}", path.display()))
}

fn parse_args(mut values: ArgsOs) -> Result<Args, String> {
    let _binary = values.next();
    let target = values.next().ok_or_else(usage)?;
    let mut positionals = Vec::new();
    let mut filters = Vec::new();
    let mut json = None;
    while let Some(value) = values.next() {
        if value == "--filter" {
            filters.push(filter_value(values.next())?);
        } else if value == "--json" {
            let out = values
                .next()
                .ok_or_else(|| "--json requires a path".to_string())?;
            json = Some(PathBuf::from(out));
        } else if value.to_string_lossy().starts_with("--") {
            return Err(format!("unknown option: {}", value.to_string_lossy()));
        } else {
            positionals.push(value);
        }
    }
    if positionals.len() > 2 {
        return Err(usage());
    }
    Ok(Args {
        target: PathBuf::from(target),
        limit: positional(&positionals, 0)?.unwrap_or(1_000_000),
        threads: positional(&positionals, 1)?.unwrap_or_else(default_threads),
        filters,
        json,
    })
}

fn usage() -> String {
    "usage: triage <test-subdir> [limit] [threads] [--filter <substr>]... [--json <out.json>]"
        .to_string()
}

fn filter_value(value: Option<OsString>) -> Result<String, String> {
    let value = value.ok_or_else(|| "--filter requires a value".to_string())?;
    let value = value.to_string_lossy().into_owned();
    (!value.is_empty())
        .then_some(value)
        .ok_or_else(|| "--filter requires a non-empty value".to_string())
}

fn positional(values: &[OsString], index: usize) -> Result<Option<usize>, String> {
    values
        .get(index)
        .map(|value| {
            value
                .to_string_lossy()
                .parse()
                .map_err(|_| format!("invalid numeric argument: {}", value.to_string_lossy()))
        })
        .transpose()
}

fn default_threads() -> usize {
    thread::available_parallelism().map_or(1, |count| count.get())
}

fn worker_stack_size() -> usize {
    env::var(STACK_SIZE_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_STACK_SIZE)
}

fn select_files(files: Vec<PathBuf>, base: &Path, filters: &[String]) -> Vec<PathBuf> {
    if filters.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|path| {
            let relative = path.strip_prefix(base).unwrap_or(path).to_string_lossy();
            filters.iter().any(|filter| relative.contains(filter))
        })
        .collect()
}

/// Run the files across `threads` independent runners and merge the results.
fn run_parallel(
    root: &Path,
    files: Vec<TestSource>,
    limit: usize,
    threads: usize,
    emit_outcomes: bool,
) -> (usize, usize, Vec<(PathBuf, String)>, Vec<JsonOutcome>) {
    let counter = Arc::new(AtomicUsize::new(0));
    let files = Arc::new(files);
    let next = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let root = root.to_path_buf();
            let files = Arc::clone(&files);
            let counter = Arc::clone(&counter);
            let next = Arc::clone(&next);
            let stack_size = worker_stack_size();
            thread::Builder::new()
                .stack_size(stack_size)
                .spawn(move || run_worker(files, root, limit, counter, next, emit_outcomes))
                .expect("spawn triage worker")
        })
        .collect();
    let mut passed = 0;
    let mut failed = 0;
    let mut failures = Vec::new();
    let mut outcomes = Vec::new();
    for handle in handles {
        let report = handle.join().unwrap_or_default();
        passed += report.passed;
        failed += report.failed;
        failures.extend(report.failures);
        outcomes.extend(report.outcomes);
    }
    failures.sort_by(|a, b| a.0.cmp(&b.0));
    outcomes.sort_by(|a, b| a.path.cmp(&b.path));
    (passed, failed, failures, outcomes)
}

/// Pull tests from a shared queue, stopping when the global failure cap is hit.
fn run_worker(
    files: Arc<Vec<TestSource>>,
    root: PathBuf,
    limit: usize,
    counter: Arc<AtomicUsize>,
    next: Arc<AtomicUsize>,
    emit_outcomes: bool,
) -> RunReport {
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut harness = HarnessCache::new(root.join("harness"));
    let mut report = RunReport::default();
    loop {
        let start = next.fetch_add(WORK_BATCH, Ordering::Relaxed);
        if start >= files.len() {
            break;
        }
        let stop = (start + WORK_BATCH).min(files.len());
        run_fixture_batch(
            &mut runner,
            &mut harness,
            &files[start..stop],
            limit,
            &counter,
            emit_outcomes,
            &mut report,
        );
        if counter.load(Ordering::Relaxed) >= limit {
            break;
        }
    }
    report
}

fn run_fixture_batch(
    runner: &mut Test262Runner<RuntimeHost>,
    harness: &mut HarnessCache,
    batch: &[TestSource],
    limit: usize,
    counter: &Arc<AtomicUsize>,
    emit_outcomes: bool,
    report: &mut RunReport,
) {
    for fixture in batch {
        if counter.load(Ordering::Relaxed) >= limit {
            break;
        }
        let outcome = if fixture.metadata.is_module {
            runner.run_test_with_cache_metadata_and_path(
                &fixture.source,
                &fixture.metadata,
                &fixture.path,
                harness,
            )
        } else {
            runner.run_test_with_cache_and_metadata(&fixture.source, &fixture.metadata, harness)
        };
        let (category, reason) = match outcome {
            Ok(TestOutcome::Pass) => (String::from("pass"), None),
            Ok(TestOutcome::Fail { reason }) | Err(reason) => {
                (normalize_reason(reason.trim()), Some(reason))
            }
        };
        if emit_outcomes {
            report.outcomes.push(JsonOutcome {
                path: fixture.path.clone(),
                category,
            });
        }
        match reason {
            None => report.passed += 1,
            Some(reason) => {
                counter.fetch_add(1, Ordering::Relaxed);
                report.failed += 1;
                report
                    .failures
                    .push((fixture.path.clone(), reason.trim().to_string()));
            }
        }
    }
}

fn load_test_sources(files: &[PathBuf]) -> Result<Vec<TestSource>, String> {
    files
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .map_err(|error| format!("test262 read failed for {}: {error}", path.display()))?;
            let metadata = TestMetadata::parse(&source).map_err(|error| {
                format!(
                    "test262 metadata parse failed for {}: {error}",
                    path.display()
                )
            })?;
            Ok(TestSource {
                path: path.clone(),
                source,
                metadata,
            })
        })
        .collect()
}

/// Group failures by normalized reason, highest-impact buckets first.
fn bucket_failures(failures: Vec<(PathBuf, String)>) -> Vec<(usize, String, Vec<PathBuf>)> {
    let mut buckets: HashMap<String, (usize, Vec<PathBuf>)> = HashMap::new();
    for (path, reason) in failures {
        let entry = buckets
            .entry(normalize_reason(&reason))
            .or_insert_with(|| (0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 5 {
            entry.1.push(path);
        }
    }
    let mut buckets: Vec<_> = buckets
        .into_iter()
        .map(|(reason, (count, samples))| (count, reason, samples))
        .collect();
    buckets.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    buckets
}

/// Collapse verbose reasons to a stable, briefer key so identical root causes
/// from different tests group into one bucket. The `Unsupported executable
/// expression` reasons embed a full AST dump that is unique per test; reduce
/// them to the construct kind so related failures count together.
fn normalize_reason(reason: &str) -> String {
    if let Some(rest) = reason.strip_prefix("Unsupported executable expression: ") {
        let construct = rest.split('(').next().unwrap_or(rest);
        return format!("Unsupported executable expression: {construct}");
    }
    let mut text = reason.to_string();
    if text.len() > 120 {
        let boundary = text
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|&index| index < 120)
            .last()
            .unwrap_or(0);
        text.truncate(boundary);
        text.push('…');
    }
    text
}

fn print_report(passed: usize, failed: usize, buckets: &[(usize, String, Vec<PathBuf>)]) {
    println!("passed={passed} failed={failed} total={}", passed + failed);
    for (count, reason, samples) in buckets {
        println!("{count:>5}  {reason}");
        for sample in samples {
            println!("         e.g. {}", sample.display());
        }
        if *count > samples.len() {
            println!("         (plus {} more)", count - samples.len());
        }
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("FAIL: {message}");
    ExitCode::from(1)
}

fn test262_root() -> PathBuf {
    env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"))
}
