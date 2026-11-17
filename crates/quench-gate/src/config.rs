//! What a gate run was asked, and where the repository and its build output are.

use std::{
    env,
    ffi::OsStr,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process::Command,
    thread,
};

/// The cargo profile every gate build and test uses: the repository's dev-loop profile.
pub const PROFILE: &str = "experiment";
const TARGET_VARIABLE: &str = "CARGO_TARGET_DIR";
const DEFAULT_TARGET: &str = "target";
const QUICK_FLAG: &str = "--quick";
const RECORD_FLAG: &str = "--record";
const JOBS_FLAG: &str = "--jobs";
const PASSED_FLAG: &str = "--passed";
/// How many directories above this crate's manifest the workspace root lies (`crates/quench-gate`).
const ROOT_DEPTH: usize = 2;

#[derive(Clone, Debug)]
pub struct Config {
    /// Parallel jobs for cargo and for the conformance runs.
    pub jobs: usize,
    /// Only the lint, census and C-parity steps.
    pub quick: bool,
    /// Write ratchet entries that have no value yet (a recorded value is never changed).
    pub record: bool,
    /// Only answer whether the checked-out tree has a full pass record.
    pub passed: bool,
    pub root: PathBuf,
    /// Cargo's target directory.
    pub target: PathBuf,
}

impl Config {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(ROOT_DEPTH).map(Path::to_path_buf).ok_or("the gate crate lies outside its workspace")?;
        let target = root.join(env::var_os(TARGET_VARIABLE).unwrap_or_else(|| DEFAULT_TARGET.into()));
        let jobs = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        let mut config = Self { jobs, quick: false, record: false, passed: false, root, target };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                QUICK_FLAG => config.quick = true,
                RECORD_FLAG => config.record = true,
                PASSED_FLAG => config.passed = true,
                JOBS_FLAG => config.jobs = args.next().and_then(|jobs| jobs.parse().ok()).filter(|jobs| *jobs > 0).ok_or_else(|| format!("{JOBS_FLAG} needs a positive number"))?,
                other => return Err(format!("unknown argument `{other}`; usage: quench-gate [{QUICK_FLAG}] [{JOBS_FLAG} N] [{RECORD_FLAG}] [{PASSED_FLAG}]")),
            }
        }
        Ok(config)
    }

    /// The binary `name` the profile builds, or what to do when it is not built.
    pub fn binary(&self, name: &str) -> Result<PathBuf, String> {
        let path = self.target.join(PROFILE).join(name);
        if path.exists() {
            return Ok(path);
        }
        Err(format!("{} is not built: run the gate without --quick, or `cargo build --profile {PROFILE} --bin {name}`", path.display()))
    }

    /// A command that runs in the repository root.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        command.current_dir(&self.root);
        command
    }

    /// `cargo <subcommand> --profile experiment -j<jobs>` in the repository root.
    pub fn cargo(&self, subcommand: &str) -> Command {
        self.cargo_in(subcommand, PROFILE)
    }

    /// `cargo <subcommand> --profile <profile> -j<jobs>` in the repository root.
    pub fn cargo_in(&self, subcommand: &str, profile: &str) -> Command {
        let mut command = self.command("cargo");
        command.arg(subcommand).args(["--profile", profile]).arg(format!("-j{}", self.jobs));
        command
    }
}
