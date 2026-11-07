//! `--test`: Node's test coordinator. Each test file runs in its own child process, which
//! streams its completed reports back as JSON lines; the coordinator renumbers, renders and
//! counts them, and reports a file whose process failed outside any test.
use super::report::{Directive, Failure, Kind, Location, Reporter, Style, TestReport, Yaml};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Instant;

/// Node's environment marker for a test file run by the coordinator.
pub(crate) const CHILD_CONTEXT_VARIABLE: &str = "NODE_TEST_CONTEXT";
pub(crate) const CHILD_CONTEXT: &str = "child-v8";
/// Starts a child's report line; anything else it prints is the test file's own output.
const REPORT_PREFIX: &str = "\u{1}quench:test:report\u{1}";
/// Node's default test file patterns, relative to the working directory.
const DEFAULT_PATTERNS: [&str; 6] = [
    "**/*.test.{cjs,mjs,js}",
    "**/*-test.{cjs,mjs,js}",
    "**/*_test.{cjs,mjs,js}",
    "**/test-*.{cjs,mjs,js}",
    "**/test.{cjs,mjs,js}",
    "**/test/**/*.{cjs,mjs,js}",
];
/// Options the coordinator consumes; a child gets the rest of execArgv (`--test-only`...).
const COORDINATOR_OPTIONS: [&str; 6] = [
    "--test",
    "--test-reporter",
    "--test-reporter-destination",
    "--test-concurrency",
    "--test-isolation",
    "--test-shard",
];
const IGNORED_DIRECTORY: &str = "node_modules";
const GLOB_CHARACTERS: [char; 4] = ['*', '?', '{', '['];
const FILE_FAILURE: &str = "test failed";
use quench_runtime::units::MILLISECONDS_PER_SECOND;

pub(crate) fn stream_line(report: &TestReport) -> String {
    format!("{REPORT_PREFIX}{}\n", serde_json::to_string(report).unwrap_or_default())
}

/// What a child process sent: a completed top-level test, or its own output.
enum Event {
    Report(Box<TestReport>),
    Output(String),
}

pub(crate) struct FileRun {
    pub(crate) file: String,
    pub(crate) reports: Vec<TestReport>,
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

/// The coordinator's options that are not the child's (Node strips them from execArgv).
fn child_arguments(exec_argv: &[String]) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut skip_value = false;
    for argument in exec_argv {
        if std::mem::take(&mut skip_value) {
            continue;
        }
        let name = argument.split_once('=').map_or(argument.as_str(), |(name, _)| name);
        if COORDINATOR_OPTIONS.contains(&name) {
            skip_value = !argument.contains('=') && crate::modules::cli_options::takes_string_value(argument);
            continue;
        }
        arguments.push(argument.clone());
    }
    arguments
}

/// Run `node --test [patterns]`.
pub(crate) fn run(patterns: &[String], exec_argv: &[String], reporter_option: Option<&str>) -> ExitCode {
    let files = match resolve_patterns(patterns) {
        Ok(files) => files,
        Err(missing) => {
            eprintln!("Could not find '{missing}'");
            return ExitCode::FAILURE;
        }
    };
    let terminal = crate::modules::tty_state::is_terminal_fd(1);
    let style = match Style::select(reporter_option, terminal) {
        Ok(style) => style,
        Err(name) => {
            eprintln!("node:test reporter '{name}' is not supported");
            return ExitCode::FAILURE;
        }
    };
    let started = Instant::now();
    let mut reporter = Reporter::new(style, cwd().to_string_lossy().into_owned());
    let arguments = child_arguments(exec_argv);
    let mut stdout = std::io::stdout();
    let mut top_level = 0;
    for file in &files {
        execute(file, &arguments, |event| {
            let text = match event {
                Event::Report(mut report) => {
                    top_level += 1;
                    report.number = top_level;
                    reporter.test(&report)
                }
                Event::Output(text) => reporter.output(&text),
            };
            let _ = stdout.write_all(text.as_bytes());
            let _ = stdout.flush();
        });
    }
    let summary = reporter.summary(&[], started.elapsed().as_secs_f64() * MILLISECONDS_PER_SECOND);
    let _ = stdout.write_all(summary.as_bytes());
    let _ = stdout.flush();
    if reporter.counters().success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `run({ files })`: each file's reports, renumbered across the run.
pub(crate) fn run_files(files: &[String]) -> Vec<FileRun> {
    let mut top_level = 0;
    files
        .iter()
        .map(|file| {
            let mut reports = Vec::new();
            execute(file, &[], |event| {
                if let Event::Report(mut report) = event {
                    top_level += 1;
                    report.number = top_level;
                    reports.push(*report);
                }
            });
            FileRun { file: file.clone(), reports }
        })
        .collect()
}

/// `run()`'s files resolved against the working directory; missing ones are dropped.
pub(crate) fn resolve_files(files: &[String]) -> Vec<String> {
    files.iter().filter_map(|file| resolve_patterns(std::slice::from_ref(file)).ok()).flatten().collect()
}

fn executable() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("quench-node"))
}

/// Run one test file and deliver its events in order; a process that failed without a
/// failing test becomes a failing test named after the file.
fn execute(file: &str, arguments: &[String], mut deliver: impl FnMut(Event)) {
    let started = Instant::now();
    let child = Command::new(executable())
        .args(arguments)
        .arg(file)
        .env(CHILD_CONTEXT_VARIABLE, CHILD_CONTEXT)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            deliver(Event::Output(format!("{error}\n")));
            return;
        }
    };
    let stderr = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = stderr.read_to_end(&mut text);
            String::from_utf8_lossy(&text).into_owned()
        })
    });
    let mut failed = false;
    if let Some(stdout) = child.stdout.take() {
        let mut stdout = std::io::BufReader::new(stdout);
        let mut line = Vec::new();
        while stdout.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
            let text = String::from_utf8_lossy(&line).into_owned();
            line.clear();
            let report = text
                .strip_prefix(REPORT_PREFIX)
                .and_then(|json| serde_json::from_str::<TestReport>(json.trim_end()).ok());
            match report {
                Some(report) => {
                    failed |= !report.passed && !matches!(report.directive, Some(Directive::Todo(_)));
                    deliver(Event::Report(Box::new(report)));
                }
                None => deliver(Event::Output(text)),
            }
        }
    }
    let status = child.wait();
    if let Some(text) = stderr.and_then(|thread| thread.join().ok()).filter(|text| !text.is_empty()) {
        deliver(Event::Output(text));
    }
    let (code, signal) = match &status {
        Ok(status) => (status.code(), crate::modules::child_process_shared_vm::signal_name(status).map(str::to_owned)),
        Err(_) => (None, None),
    };
    if failed || (code == Some(0) && signal.is_none()) {
        return;
    }
    deliver(Event::Report(Box::new(file_failure(file, code, signal, started))));
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "\\'"))
}

fn file_failure(file: &str, code: Option<i32>, signal: Option<String>, started: Instant) -> TestReport {
    let name = relative(file);
    let fields = vec![
        ("failureType".to_owned(), Yaml::Scalar(quoted("testCodeFailure"))),
        ("exitCode".to_owned(), code.map_or(Yaml::Null, |code| Yaml::Scalar(code.to_string()))),
        ("signal".to_owned(), signal.as_deref().map_or(Yaml::Null, |signal| Yaml::Scalar(quoted(signal)))),
        ("error".to_owned(), Yaml::Scalar(quoted(FILE_FAILURE))),
        ("code".to_owned(), Yaml::Scalar(quoted("ERR_TEST_FAILURE"))),
    ];
    TestReport {
        name,
        nesting: 0,
        number: 0,
        kind: Kind::Test,
        directive: None,
        passed: false,
        cancelled: false,
        duration_ms: started.elapsed().as_secs_f64() * MILLISECONDS_PER_SECOND,
        location: Some(Location { file: file.to_owned(), line: 1, column: 1 }),
        failure: Some(Failure {
            failure_type: "testCodeFailure".to_owned(),
            fields,
            inspected: quoted(FILE_FAILURE),
            inspected_colored: Some(format!("\x1b[32m{}\x1b[39m", quoted(FILE_FAILURE))),
        }),
        children: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn relative(file: &str) -> String {
    let base = cwd();
    Path::new(file).strip_prefix(&base).map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|_| file.to_owned())
}

/// Node's `createTestFileList`: explicit paths as given, globs matched under the working
/// directory, and Node's default patterns when none are given; sorted and deduplicated.
fn resolve_patterns(patterns: &[String]) -> Result<Vec<String>, String> {
    let base = cwd();
    let mut files = Vec::new();
    let default_patterns = DEFAULT_PATTERNS.iter().map(|pattern| (*pattern).to_owned()).collect::<Vec<_>>();
    let patterns = if patterns.is_empty() { &default_patterns } else { patterns };
    for pattern in patterns {
        if !pattern.contains(GLOB_CHARACTERS) {
            let path = base.join(pattern);
            if !path.exists() {
                return Err(pattern.clone());
            }
            files.push(path.to_string_lossy().into_owned());
            continue;
        }
        let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
        for expanded in expand_braces(pattern) {
            let segments = expanded.split('/').filter(|segment| !segment.is_empty()).collect::<Vec<_>>();
            walk(&base, &segments, &mut files);
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_owned()];
    };
    let Some(close) = pattern[open..].find('}').map(|offset| open + offset) else {
        return vec![pattern.to_owned()];
    };
    pattern[open + 1..close]
        .split(',')
        .flat_map(|choice| expand_braces(&format!("{}{choice}{}", &pattern[..open], &pattern[close + 1..])))
        .collect()
}

/// Match `segments` below `directory`; `**` spans any number of directories.
fn walk(directory: &Path, segments: &[&str], files: &mut Vec<String>) {
    let Some((segment, rest)) = segments.split_first() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    if *segment == "**" {
        walk(directory, rest, files);
    }
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if name.starts_with('.') || (name == IGNORED_DIRECTORY && *segment != IGNORED_DIRECTORY) {
            continue;
        }
        let path = entry.path();
        if *segment == "**" {
            if is_directory {
                walk(&path, segments, files);
            }
            continue;
        }
        if !matches(segment.as_bytes(), name.as_bytes()) {
            continue;
        }
        if rest.is_empty() {
            if !is_directory {
                files.push(path.to_string_lossy().into_owned());
            }
        } else if is_directory {
            walk(&path, rest, files);
        }
    }
}

/// One path segment against one glob segment (`*`, `?`, literal).
fn matches(pattern: &[u8], name: &[u8]) -> bool {
    match (pattern.split_first(), name.split_first()) {
        (None, None) => true,
        (Some((b'*', rest)), _) => matches(rest, name) || (!name.is_empty() && matches(pattern, &name[1..])),
        (Some((b'?', rest)), Some((_, name))) => matches(rest, name),
        (Some((expected, rest)), Some((actual, name))) => expected == actual && matches(rest, name),
        _ => false,
    }
}
