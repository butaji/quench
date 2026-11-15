//! What a gate run was asked, and where the repository and its build output are.

use std::{
    ffi::OsStr,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process::Command,
    thread,
};

/// The cargo profile every gate build and test uses: the repository's dev-loop profile.
pub const PROFILE: &str = "experiment";
const QUICK_FLAG: &str = "--quick";
const JOBS_FLAG: &str = "--jobs";
/// How many directories above this crate's manifest the workspace root lies (`crates/quench-gate`).
const ROOT_DEPTH: usize = 2;

#[derive(Clone, Debug)]
pub struct Config {
    /// Parallel jobs for cargo and for the conformance runs.
    pub jobs: usize,
    /// Only the lint, census and C-parity steps.
    pub quick: bool,
    pub root: PathBuf,
}

impl Config {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(ROOT_DEPTH).map(Path::to_path_buf).ok_or("the gate crate lies outside its workspace")?;
        let jobs = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        let mut config = Self { jobs, quick: false, root };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                QUICK_FLAG => config.quick = true,
                JOBS_FLAG => config.jobs = args.next().and_then(|jobs| jobs.parse().ok()).filter(|jobs| *jobs > 0).ok_or_else(|| format!("{JOBS_FLAG} needs a positive number"))?,
                other => return Err(format!("unknown argument `{other}`; usage: quench-gate [{QUICK_FLAG}] [{JOBS_FLAG} N]")),
            }
        }
        Ok(config)
    }

    /// A command that runs in the repository root.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        command.current_dir(&self.root);
        command
    }

    /// `cargo <subcommand> --profile experiment -j<jobs>` in the repository root.
    pub fn cargo(&self, subcommand: &str) -> Command {
        let mut command = self.command("cargo");
        command.arg(subcommand).args(["--profile", PROFILE]).arg(format!("-j{}", self.jobs));
        command
    }
}
