use quench_test262::TestMetadata;
use serde_json::json;
use std::{
    collections::HashMap,
    env::{self, ArgsOs},
    ffi::OsString,
    path::{Path, PathBuf},
};

pub const DEFAULT_STACK_SIZE: usize = 256 * 1024 * 1024;
pub const STACK_SIZE_ENV: &str = "TRIAGE_WORKER_STACK_SIZE_BYTES";
pub const WORK_BATCH: usize = 32;
pub const CRASHED_AT_RUNTIME: &[&str] = include!("crashed.rs");
pub struct TestSource {
    pub path: PathBuf,
    pub source: String,
    pub metadata: TestMetadata,
}
pub struct Args {
    pub target: PathBuf,
    pub limit: usize,
    pub threads: usize,
    pub filters: Vec<String>,
    pub json: Option<PathBuf>,
}
pub struct JsonOutcome {
    pub path: PathBuf,
    pub category: String,
}
#[derive(Default)]
pub struct RunReport {
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<(PathBuf, String)>,
    pub outcomes: Vec<JsonOutcome>,
}
pub fn parse_args(mut v: ArgsOs) -> Result<Args, String> {
    v.next();
    let target = v.next().ok_or_else(usage)?;
    let mut p = Vec::new();
    let mut f = Vec::new();
    let mut j = None;
    while let Some(x) = v.next() {
        if x == "--filter" {
            f.push(filter_value(v.next())?)
        } else if x == "--json" {
            j = Some(PathBuf::from(
                v.next()
                    .ok_or_else(|| "--json requires a path".to_string())?,
            ))
        } else if x.to_string_lossy().starts_with("--") {
            return Err(format!("unknown option: {}", x.to_string_lossy()));
        } else {
            p.push(x)
        }
    }
    if p.len() > 2 {
        return Err(usage());
    }
    Ok(Args {
        target: PathBuf::from(target),
        limit: positional(&p, 0)?.unwrap_or(1_000_000),
        threads: positional(&p, 1)?.unwrap_or_else(default_threads),
        filters: f,
        json: j,
    })
}
fn usage() -> String {
    "usage: triage <test-subdir> [limit] [threads] [--filter <substr>]... [--json <out.json>]"
        .into()
}
fn filter_value(v: Option<OsString>) -> Result<String, String> {
    let s = v
        .ok_or_else(|| String::from("--filter requires a value"))?
        .to_string_lossy()
        .into_owned();
    (!s.is_empty())
        .then_some(s)
        .ok_or_else(|| "--filter requires a non-empty value".into())
}
fn positional(v: &[OsString], i: usize) -> Result<Option<usize>, String> {
    v.get(i)
        .map(|x| {
            x.to_string_lossy()
                .parse()
                .map_err(|_| format!("invalid numeric argument: {}", x.to_string_lossy()))
        })
        .transpose()
}
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}
pub fn worker_stack_size() -> usize {
    env::var(STACK_SIZE_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_STACK_SIZE)
}
pub fn test262_root() -> PathBuf {
    env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"))
}
pub fn select_files(files: Vec<PathBuf>, base: &Path, filters: &[String]) -> Vec<PathBuf> {
    if filters.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|p| {
            let r = p.strip_prefix(base).unwrap_or(p).to_string_lossy();
            filters.iter().any(|f| r.contains(f))
        })
        .collect()
}
pub fn load_test_sources(files: &[PathBuf]) -> Result<Vec<TestSource>, String> {
    files
        .iter()
        .map(|p| {
            let source = std::fs::read_to_string(p)
                .map_err(|e| format!("test262 read failed for {}: {e}", p.display()))?;
            let metadata = TestMetadata::parse(&source)
                .map_err(|e| format!("test262 metadata parse failed for {}: {e}", p.display()))?;
            Ok(TestSource {
                path: p.clone(),
                source,
                metadata,
            })
        })
        .collect()
}
pub fn normalize_reason(r: &str) -> String {
    if let Some(x) = r.strip_prefix("Unsupported executable expression: ") {
        return format!(
            "Unsupported executable expression: {}",
            x.split('(').next().unwrap_or(x)
        );
    }
    let mut t = r.to_string();
    if t.len() > 120 {
        let n = t
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i < 120)
            .last()
            .unwrap_or(0);
        t.truncate(n);
        t.push('…')
    }
    t
}
pub fn bucket_failures(f: Vec<(PathBuf, String)>) -> Vec<(usize, String, Vec<PathBuf>)> {
    let mut b: HashMap<String, (usize, Vec<PathBuf>)> = HashMap::new();
    for (p, r) in f {
        let e = b
            .entry(normalize_reason(&r))
            .or_insert_with(|| (0, Vec::new()));
        e.0 += 1;
        if e.1.len() < 5 {
            e.1.push(p)
        }
    }
    let mut b: Vec<_> = b.into_iter().map(|(r, (n, s))| (n, r, s)).collect();
    b.sort_by(|a, z| z.0.cmp(&a.0).then_with(|| a.1.cmp(&z.1)));
    b
}
pub fn print_report(p: usize, f: usize, b: &[(usize, String, Vec<PathBuf>)]) {
    println!("passed={p} failed={f} total={}", p + f);
    for (n, r, s) in b {
        println!("{n:>5}  {r}");
        for x in s {
            println!("         e.g. {}", x.display())
        }
        if *n > s.len() {
            println!("         (plus {} more)", n - s.len())
        }
    }
}
pub fn fail(m: &str) -> std::process::ExitCode {
    eprintln!("FAIL: {m}");
    std::process::ExitCode::from(1)
}
fn read_head(p: &Path) -> String {
    let r = std::fs::read_to_string(p).unwrap_or_default();
    let r = r.trim();
    if let Some(n) = r.strip_prefix("ref: ") {
        std::fs::read_to_string(p.parent().unwrap_or_else(|| Path::new("")).join(n))
            .map(|x| x.trim().into())
            .unwrap_or_default()
    } else {
        r.into()
    }
}
pub fn write_json_report(
    p: &Path,
    a: &Args,
    r: &Path,
    pass: usize,
    fail: usize,
    o: &[JsonOutcome],
) -> Result<(), String> {
    let base = r.join("test").join(&a.target);
    let results:Vec<_>=o.iter().map(|x|Ok(json!({"fixture":x.path.strip_prefix(&base).map_err(|_|format!("outside target: {}",x.path.display()))?.to_string_lossy(),"category":x.category}))).collect::<Result<_,String>>()?;
    let tree = read_head(&r.join("HEAD"));
    let report = json!({"tool":"quench-triage","fingerprints":{"test262_tree":tree,"target":a.target.display().to_string()},"passed":pass,"failed":fail,"results":results});
    std::fs::write(p, serde_json::to_string_pretty(&report).unwrap())
        .map_err(|e| format!("write {}: {e}", p.display()))
}
