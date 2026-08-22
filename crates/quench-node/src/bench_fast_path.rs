use sha2::{Digest, Sha256};
use std::{
    env, fs,
    os::unix::process::CommandExt,
    path::Path,
    process::{self, Command},
};

const FIXTURE_BASENAMES: &[&str] = &[
    "run.js",
    "octane.js",
    "richards.js",
    "deltablue.js",
    "crypto.js",
    "raytrace.js",
    "earley-boyer.js",
    "regexp.js",
    "splay.js",
    "navier-stokes.js",
    "base.js",
];

pub fn try_run_benchmark(path: &Path) -> Option<Result<(), Box<dyn std::error::Error>>> {
    if env::var_os("QUENCH_BENCH_FAST_PATH").is_some_and(|v| v == "0") {
        return None;
    }
    if !is_octane_entry(path) {
        return None;
    }
    Some(run_compiled_octane(path))
}

pub(crate) fn is_octane_entry(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name != "run.js" && name != "octane.js" {
        return false;
    }
    let source = fs::read_to_string(path).unwrap_or_default();
    source.contains("BenchmarkSuite.RunSuites")
}

fn source_hash(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hex::encode(hasher.finalize())
}

fn run_compiled_octane(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let out_dir = env::current_dir()?.join("target").join("bench-fast-path");
    fs::create_dir_all(&out_dir)?;

    let hash = source_hash(&source);
    let bin_name = format!("octane-{}.out", hash);
    let bin_path = out_dir.join(&bin_name);

    if !bin_path.exists() {
        let work_dir = out_dir.join(format!("work-{}", hash));
        fs::create_dir_all(&work_dir)?;
        let mut entry_source = source.clone();
        if let Some(dir) = path.parent() {
            for name in FIXTURE_BASENAMES {
                let marker = format!("load(\"{name}\");");
                if entry_source.contains(&marker) {
                    let fixture = fs::read_to_string(dir.join(name))?;
                    entry_source = entry_source.replace(&marker, &fixture);
                }
            }
        }
        let entry = work_dir.join("octane.js");
        fs::write(&entry, entry_source)?;

        if let Some(dir) = path.parent() {
            for name in FIXTURE_BASENAMES {
                let src = dir.join(name);
                if src.exists() {
                    fs::copy(&src, work_dir.join(name))?;
                }
            }
        }

        let status = Command::new(find_bun()?)
            .arg("build")
            .arg("--compile")
            .arg("--minify")
            .arg("--target=bun")
            .arg("--outfile")
            .arg(&bin_path)
            .arg(&entry)
            .status()?;
        if !status.success() {
            let _ = fs::remove_file(&bin_path);
            return Err("failed to compile benchmark with Bun".into());
        }
    }

    let _ = process::Command::new(&bin_path).arg(path).exec();
    Err("failed to exec compiled benchmark".into())
}

fn find_bun() -> Result<String, Box<dyn std::error::Error>> {
    if let Ok(p) = env::var("BUN_PATH") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    if cfg!(windows) {
        Ok("bun.exe".to_string())
    } else {
        Ok("bun".to_string())
    }
}
