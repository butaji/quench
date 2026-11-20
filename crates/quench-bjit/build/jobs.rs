//! The rustc jobs of the stencil build. A shard's object is compiled from its instances (`shard_job`). An IR job runs
//! the same shard at opt-level 0 with no LLVM pass (`ir_job`) and keys the rows it defines (`ir_keys`, `ir_hash`).
//! Each job is keyed by everything it is built from (`cache::shard_key`), so a rerun whose inputs are unchanged runs
//! no rustc.

use super::{Arch, CALL_ROWS_VAR, Family, INSTANCES_VAR, ObjectRow, SOURCE, Stencil, base_args, call_rows_file, rustc, stencil_rows};
use crate::{bytes, cache, ir_hash, row_cache, shards};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

/// One rustc job: its arguments, its environment, what it leaves (`output`: the object, or an IR job's keys), and the
/// key of everything it is built from.
pub struct Shard {
    args: Vec<OsString>,
    envs: [(&'static str, PathBuf); 2],
    output: PathBuf,
    /// The shard's instance file, which its IR must not name (`ir_keys`).
    instances: PathBuf,
    /// An IR job's LLVM IR: read and removed once its keys are taken.
    ir: Option<PathBuf>,
    key_file: PathBuf,
    key: String,
    /// What an IR job's row keys cover beyond its inputs: rustc's version and the object's arguments.
    context: Vec<u8>,
}

impl Shard {
    /// Whether the output on disk was made from exactly these inputs.
    fn cached(&self) -> bool {
        self.output.is_file() && fs::read_to_string(&self.key_file).is_ok_and(|key| key == self.key)
    }
}

pub fn cpu_name(cpu: Option<&str>) -> &str {
    cpu.unwrap_or("native")
}

/// A shard's arguments beyond its instances and output: the target, the flags file's flags, the cpu and the family's cfg.
fn object_args(cpu: Option<&str>, family: Family) -> Vec<String> {
    let mut args = base_args();
    if let Some(cpu) = cpu {
        args.extend(["-C".to_string(), format!("target-cpu={cpu}")]);
    }
    args.extend(family.args());
    args
}

/// The IR of a shard's object arguments: the same target, flags and cfgs, but no optimization and no pass (the flags
/// file's `-C opt-level=3` and `-C passes` are dropped, and its `--emit`).
fn ir_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < args.len() {
        if args[at] == "-C" && args.get(at + 1).is_some_and(|value| value.starts_with("opt-level=") || value.starts_with("passes=")) {
            at += 2;
        } else if args[at].starts_with("--emit") {
            at += 1;
        } else {
            out.push(args[at].clone());
            at += 1;
        }
    }
    out.extend(["--emit=llvm-ir", "-C", "opt-level=0", "-C", "no-prepopulate-passes"].map(String::from));
    out
}

fn to_os(args: Vec<String>) -> Vec<OsString> {
    args.into_iter().map(Into::into).collect()
}

fn envs(out: &Path, instances: &Path) -> [(&'static str, PathBuf); 2] {
    [(INSTANCES_VAR, instances.to_path_buf()), (CALL_ROWS_VAR, call_rows_file(out))]
}

/// What an IR job's row keys cover beyond its shard's key: rustc's version, then the object arguments.
fn context_of(args: &[String]) -> Vec<u8> {
    let mut text = Command::new(rustc()).arg("-vV").output().map(|output| output.stdout).unwrap_or_default();
    for arg in args {
        text.push(0);
        text.extend_from_slice(arg.as_bytes());
    }
    text
}

/// The compile job of one shard's object. Not `-C incremental`: rustc's incremental mode partitions and inlines
/// differently, which changed row bytes, so an object is reused whole, by its key, or compiled whole.
pub fn shard_job(out: &Path, stem: &str, family: Family, cpu: Option<&str>, sources: u128, instances: &Path) -> Shard {
    let object = out.join(format!("{stem}.o"));
    let args = object_args(cpu, family);
    let key = cache::shard_key(sources, &args, &[instances, &call_rows_file(out)]);
    let mut command = to_os(args);
    command.extend(["-o".into(), object.clone().into(), SOURCE.into()]);
    Shard { args: command, envs: envs(out, instances), key_file: object.with_extension("key"), output: object, instances: instances.to_path_buf(), ir: None, key, context: Vec::new() }
}

/// The IR job of one shard: its LLVM IR before any LLVM pass, read by `ir_keys`.
pub fn ir_job(out: &Path, stem: &str, family: Family, cpu: Option<&str>, sources: u128, instances: &Path) -> Shard {
    let args = object_args(cpu, family);
    let ir = ir_args(&args);
    let keys = out.join(format!("{stem}.keys"));
    let llvm = out.join(format!("{stem}.ll"));
    let key = cache::shard_key(sources, &ir, &[instances, &call_rows_file(out)]);
    let context = context_of(&args);
    let mut command = to_os(ir);
    command.extend(["-o".into(), llvm.clone().into(), SOURCE.into()]);
    Shard { args: command, envs: envs(out, instances), key_file: keys.with_extension("key"), output: keys, instances: instances.to_path_buf(), ir: Some(llvm), key, context }
}

fn run_rustc(job: &Shard) {
    let status = Command::new(rustc()).args(&job.args).envs(job.envs.iter().map(|(name, path)| (*name, path))).status().expect("run rustc");
    assert!(status.success(), "rustc failed on {SOURCE} ({})", job.output.display());
}

/// The rows an IR job's shard defines with their keys: read from the keys file when the job is cached, else by running
/// rustc and keying the rows of its LLVM IR (`ir_hash`).
pub fn ir_keys(job: &Shard) -> Vec<(String, u128)> {
    if job.cached() {
        return row_cache::read_keys(&job.output);
    }
    let _ = fs::remove_file(&job.key_file);
    run_rustc(job);
    let llvm = job.ir.as_ref().expect("an IR job's LLVM IR");
    let text = fs::read_to_string(llvm).unwrap_or_else(|e| panic!("read {}: {e}", llvm.display()));
    // A row whose panic location is in its instance file would key on the shard's file and line, which move with the
    // partition: that is refused here rather than keyed.
    let instances = job.instances.file_name().and_then(|name| name.to_str()).expect("an instance file name");
    assert!(!text.contains(instances), "{instances}: a row's panic location names its instance file, so its key would depend on the shard");
    let keys = ir_hash::row_keys(&text, &job.context, bytes::is_row_name);
    drop(text);
    fs::remove_file(llvm).unwrap();
    row_cache::write_keys(&job.output, &keys);
    fs::write(&job.key_file, &job.key).unwrap();
    keys
}

/// The rows of every shard object in `jobs`, each compiled unless cached, checked and laid out (`stencil_rows`).
pub fn objects(jobs: &[&Shard], arch: Arch, read: fn(&[u8]) -> Vec<ObjectRow>) -> Vec<Stencil> {
    pool(jobs, |job| {
        if !job.cached() {
            let _ = fs::remove_file(&job.key_file);
            run_rustc(job);
            fs::write(&job.key_file, &job.key).unwrap();
        }
        stencil_rows(arch, read(&fs::read(&job.output).unwrap()))
    })
    .into_iter()
    .flatten()
    .collect()
}

/// Runs `work` on every job, on as many threads as `shards::workers` allows (`QUENCH_STENCIL_WORKERS` caps them), and
/// returns the results in job order.
pub fn pool<T: Send>(jobs: &[&Shard], work: impl Fn(&Shard) -> T + Sync) -> Vec<T> {
    let parallelism = env::var("QUENCH_STENCIL_WORKERS").ok().and_then(|count| count.parse().ok()).unwrap_or_else(|| thread::available_parallelism().map_or(1, std::num::NonZero::get));
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, T)> = thread::scope(|scope| {
        let workers: Vec<_> = (0..shards::workers(jobs.len(), parallelism))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&job) = jobs.get(at) else { return done };
                        done.push((at, work(job)));
                    }
                })
            })
            .collect();
        workers.into_iter().flat_map(|worker| worker.join().expect("a job worker")).collect()
    });
    done.sort_by_key(|(at, _)| *at);
    done.into_iter().map(|(_, result)| result).collect()
}
