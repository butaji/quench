use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use quench_test262::{discover_js_files, RuntimeHost, Test262Runner, TestOutcome};

type Categories = BTreeMap<String, (usize, Vec<PathBuf>)>;
type TriageResult = (usize, usize, Categories);

fn main() -> ExitCode {
    let Some(target) = env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: triage <test-subdir> [limit]");
        return ExitCode::from(2);
    };
    let limit = env::args()
        .nth(2)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1_000_000);
    let root = test262_root();
    let base = root.join("test").join(&target);
    let files = match discover_js_files(&base) {
        Ok(files) => files,
        Err(error) => return fail(&format!("discover: {error}")),
    };
    let (passed, failed, categories) = execute_files(&root, files, limit);
    println!("passed={passed} failed={failed}");
    for (category, (count, sample)) in &categories {
        let file = sample
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        println!("  {count:>5}  {category}");
        println!("           e.g. {file}");
    }
    ExitCode::SUCCESS
}

fn execute_files(root: &Path, files: Vec<PathBuf>, limit: usize) -> TriageResult {
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut categories = BTreeMap::new();
    let mut passed = 0;
    let mut failed = 0;
    for path in files {
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let outcome = runner.run_test_with_harness(&source, |name| {
            fs::read_to_string(root.join("harness").join(name))
                .map_err(|error| format!("harness {name}: {error}"))
        });
        match outcome {
            Ok(TestOutcome::Pass) => passed += 1,
            Ok(TestOutcome::Fail { reason }) => {
                failed += 1;
                record(&mut categories, reason.trim().to_string(), &path);
            }
            Err(error) => {
                failed += 1;
                record(&mut categories, error.trim().to_string(), &path);
            }
        }
        if failed >= limit {
            break;
        }
    }
    (passed, failed, categories)
}

fn record(categories: &mut BTreeMap<String, (usize, Vec<PathBuf>)>, reason: String, path: &Path) {
    let category = categorize(&reason);
    let entry = categories
        .entry(category)
        .or_insert_with(|| (0, Vec::new()));
    entry.0 += 1;
    if entry.1.len() < 3 {
        entry.1.push(path.to_path_buf());
    }
}

fn categorize(reason: &str) -> String {
    for marker in [
        "SyntaxError",
        "Unsupported",
        "Residual VM error",
        "NotCallable",
        "RegisterOutOfBounds",
        "expected",
    ] {
        if reason.contains(marker) {
            return shorten(marker);
        }
    }
    shorten(reason)
}

fn shorten(reason: &str) -> String {
    let mut text = reason.to_string();
    if text.len() > 120 {
        text.truncate(120);
        text.push('…');
    }
    text
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
