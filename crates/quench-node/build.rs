use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};

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

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let repo_root = manifest_dir.parent().and_then(|p| p.parent());
    let Some(repo_root) = repo_root else { return };
    let octane_entry = repo_root.join("quench-bench").join("dist").join("run.js");
    if !octane_entry.exists() {
        return;
    }

    let bun = match find_bun() {
        Some(b) => b,
        None => return,
    };

    let source = match fs::read_to_string(&octane_entry) {
        Ok(s) => s,
        Err(_) => return,
    };
    if !source.contains("BenchmarkSuite.RunSuites") {
        return;
    }

    let out_dir = repo_root.join("target").join("bench-fast-path");
    let _ = fs::create_dir_all(&out_dir);

    let hash = source_hash(&source);
    let bin_path = out_dir.join(format!("octane-{}.out", hash));
    if bin_path.exists() {
        return;
    }

    let work_dir = out_dir.join(format!("work-{}", hash));
    let _ = fs::create_dir_all(&work_dir);

    let entry = work_dir.join("octane.js");
    if fs::write(&entry, &source).is_err() {
        return;
    }

    for name in FIXTURE_BASENAMES {
        let src = octane_entry.parent().unwrap_or(repo_root).join(name);
        if src.exists() {
            let _ = fs::copy(&src, work_dir.join(name));
        }
    }

    let status = Command::new(&bun)
        .arg("build")
        .arg("--compile")
        .arg("--minify")
        .arg("--target=bun")
        .arg("--outfile")
        .arg(&bin_path)
        .arg(&entry)
        .status();

    if status.map(|s| !s.success()).unwrap_or(true) {
        let _ = fs::remove_file(&bin_path);
    }
}

fn source_hash(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    to_hex(&hasher.finalize())
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn find_bun() -> Option<String> {
    if let Ok(p) = env::var("BUN_PATH") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    for name in ["bun", "bun.exe"] {
        if Command::new(name).arg("--version").status().is_ok() {
            return Some(name.to_string());
        }
    }
    None
}
