//! # Build Driver
//!
//! Main compilation orchestration.

use std::path::PathBuf;
use std::process::Command as ProcCommand;
use crate::{Result, parser, analyzer, codegen};
use super::config::RuneConfig;
use super::cache::CacheManager;

/// Build mode.
#[derive(Debug, Clone, Copy, Default)]
pub enum BuildMode {
    /// Development with hot reload
    #[default]
    Dev,
    /// Release build
    Release,
}

/// Options for building.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Mode to build in
    pub mode: BuildMode,
    /// Working directory
    pub workspace: PathBuf,
    /// Target crate name
    pub target_crate: Option<String>,
    /// Config file path
    pub config: Option<PathBuf>,
    /// File to transpile (for transpile command)
    pub transpile_file: Option<PathBuf>,
    /// Verbose output
    pub verbose: bool,
}

impl BuildOptions {
    /// Create new options.
    #[must_use]
    pub const fn new(workspace: PathBuf) -> Self {
        Self {
            mode: BuildMode::Dev,
            workspace,
            target_crate: None,
            config: None,
            transpile_file: None,
            verbose: false,
        }
    }

    /// Set development mode.
    #[must_use]
    pub const fn dev(mut self) -> Self {
        self.mode = BuildMode::Dev;
        self
    }

    /// Set release mode.
    #[must_use]
    pub const fn release(mut self) -> Self {
        self.mode = BuildMode::Release;
        self
    }
}

/// Main build driver.
pub struct BuildDriver {
    options: BuildOptions,
    config: RuneConfig,
    cache: CacheManager,
}

impl BuildDriver {
    /// Create a new build driver.
    ///
    /// # Errors
    /// Returns an error if configuration or cache cannot be initialized.
    pub fn new(options: BuildOptions) -> Result<Self> {
        let config_path = options.config.clone()
            .or_else(|| Some(options.workspace.join("rune.toml")));

        let config = if let Some(ref path) = config_path {
            if path.exists() {
                RuneConfig::load(path)?
            } else {
                RuneConfig::default()
            }
        } else {
            RuneConfig::default()
        };

        let cache = CacheManager::new(&options.workspace)?;

        Ok(Self {
            options,
            config,
            cache,
        })
    }

    /// Run in development mode with hot reload.
    ///
    /// # Errors
    /// Returns an error if build fails.
    pub fn dev(&mut self) -> Result<()> {
        if self.options.verbose {
            println!("Running in development mode with hot reload...");
        }

        let sources = self.find_sources()?;
        if self.options.verbose {
            println!("Found {} Rune source files", sources.len());
        }

        let parsed = Self::parse_sources(&sources)?;
        let analyses = Self::analyze_sources(&parsed)?;
        let generated = self.generate_code(&parsed, &analyses)?;
        self.write_generated(&generated)?;
        self.build_crate(false)?;
        self.setup_hot_reload()?;

        if self.options.verbose {
            println!("Development build complete. Dylib ready.");
        }

        Ok(())
    }

    /// Run in release mode.
    ///
    /// # Errors
    /// Returns an error if build fails.
    pub fn build(&mut self) -> Result<()> {
        if self.options.verbose {
            println!("Running in release mode...");
        }

        let sources = self.find_sources()?;
        let parsed = Self::parse_sources(&sources)?;
        let analyses = Self::analyze_sources(&parsed)?;
        let generated = self.generate_code(&parsed, &analyses)?;
        self.write_generated(&generated)?;
        self.build_crate(true)?;

        if self.options.verbose {
            println!("Release build complete.");
        }

        Ok(())
    }

    /// Type check only.
    ///
    /// # Errors
    /// Returns an error if analysis fails.
    pub fn check(&mut self) -> Result<()> {
        let sources = self.find_sources()?;
        let parsed = Self::parse_sources(&sources)?;
        let analyses = Self::analyze_sources(&parsed)?;

        for analysis in &analyses {
            for warning in &analysis.warnings {
                println!("warning: {} at {}", warning.message, warning.location);
            }
        }

        println!("Type check passed for {} files.", parsed.len());
        Ok(())
    }

    /// Transpile a single file to stdout.
    ///
    /// # Errors
    /// Returns an error if transpilation fails.
    pub fn transpile(&mut self) -> Result<()> {
        let file = self.options.transpile_file.as_ref()
            .ok_or_else(|| crate::RuneError::Codegen("No file specified".into()))?;

        let source = parser::parse_file(file)?;
        let analysis = analyzer::analyze(&source)?;
        let module = codegen::generate(&source, &analysis)?;

        println!("{}", module.source);
        Ok(())
    }

    /// Initialize a new project.
    ///
    /// # Errors
    /// Returns an error if project creation fails.
    #[allow(clippy::too_many_lines)]
    pub fn init(&mut self) -> Result<()> {
        let project_name = self.config.project.name.clone();
        let target_crate = self.config.build.target_crate.clone();

        let base = self.options.workspace.join("crates").join(&target_crate);
        std::fs::create_dir_all(base.join("src/native"))?;
        std::fs::create_dir_all(base.join("src/views"))?;

        let cargo = format!(r#"[package]
name = "{target_crate}"
version = "0.1.0"
edition = "2021"

[lib]
name = "{target_crate}"
path = "src/lib.rs"

[dependencies]
protocol = {{ path = "../protocol" }}
ratatui = "0.26"

[build-dependencies]
rune = {{ path = "../../.." }}
"#);

        std::fs::write(base.join("Cargo.toml"), cargo)?;

        let lib = r#"mod native;

pub struct AppState {
    pub tasks: Vec<Task>,
}

#[derive(Clone)]
pub struct Task {
    pub id: i32,
    pub title: String,
    pub done: bool,
}

#[no_mangle]
pub extern "C" fn create_app() -> *mut dyn protocol::App {
    Box::into_raw(Box::new(AppImpl))
}

struct AppImpl;

impl protocol::App for AppImpl {
    fn update(&mut self, _state: &mut AppState) {}
    fn render(&self, _term: &mut impl ratatui::backend::Backend, _state: &AppState) {}
}
"#;

        std::fs::write(base.join("src/lib.rs"), lib)?;

        let main_ts = "export function update(state: AppState): void {}\n\
export function render(state: AppState): void {}\n";

        std::fs::write(base.join("src/main.r.ts"), main_ts)?;

        let state_ts = "export type Task = { id: number; title: string; done: boolean };\n\
export type AppState = { tasks: Task[]; selected: number };\n";

        std::fs::write(base.join("src/state.r.ts"), state_ts)?;

        let native_mod = "pub mod fast_math;\n";
        std::fs::write(base.join("src/native/mod.rs"), native_mod)?;

        let fast_math = "pub fn fast_sqrt(x: f64) -> f64 { x.sqrt() }\n";
        std::fs::write(base.join("src/native/fast_math.rs"), fast_math)?;

        println!("Initialized Rune project: {project_name}");
        println!("Created structure in crates/{target_crate}/");

        Ok(())
    }

    fn find_sources(&self) -> Result<Vec<PathBuf>> {
        let src_dir = self.options.workspace
            .join("crates")
            .join(&self.config.build.target_crate)
            .join("src");

        parser::scan_directory(&src_dir)
    }

    fn parse_sources(sources: &[PathBuf]) -> Result<Vec<parser::SourceFile>> {
        sources.iter().map(|s| parser::parse_file(s.as_path())).collect()
    }

    fn analyze_sources(sources: &[parser::SourceFile]) -> Result<Vec<analyzer::AnalysisResult>> {
        sources.iter().map(analyzer::analyze).collect()
    }

    fn generate_code(
        &self,
        sources: &[parser::SourceFile],
        analyses: &[analyzer::AnalysisResult],
    ) -> Result<Vec<codegen::GeneratedModule>> {
        sources.iter()
            .zip(analyses.iter())
            .map(|(s, a)| codegen::generate(s, a))
            .collect()
    }

    fn write_generated(&self, modules: &[codegen::GeneratedModule]) -> Result<()> {
        let cache_dir = self.cache.generated_dir();

        for module in modules {
            let rel_path = module.name.replace(".r", "");
            let out_path = cache_dir.join(format!("{rel_path}.rs"));
            std::fs::create_dir_all(out_path.parent().unwrap())?;
            std::fs::write(&out_path, &module.source)?;
        }

        Ok(())
    }

    fn build_crate(&self, release: bool) -> Result<()> {
        let manifest = self.cache.generated_cargo_toml();
        let mut cmd = ProcCommand::new("cargo");
        cmd.arg("build");
        if release {
            cmd.arg("--release");
        }
        cmd.arg("--manifest-path").arg(&manifest);

        if !self.options.verbose {
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
        }

        let output = cmd.output()
            .map_err(|e| crate::RuneError::Cargo(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(crate::RuneError::Cargo(stderr.to_string()));
        }

        Ok(())
    }

    #[allow(clippy::unnecessary_wraps)]
    fn setup_hot_reload(&self) -> Result<()> {
        let hot_dir = self.options.workspace.join("target/hot");
        std::fs::create_dir_all(&hot_dir)?;

        let profile = match self.options.mode {
            BuildMode::Dev => "debug",
            BuildMode::Release => "release",
        };

        let target_crate = &self.config.build.target_crate;
        let artifact_name = format!("lib{target_crate}.so");

        let artifact = self.options.workspace
            .join("target")
            .join(profile)
            .join(&artifact_name);

        if artifact.exists() {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis();

            let safe_name = target_crate.replace('-', "_");
            let hot_name = format!("{safe_name}_{timestamp}.so");
            let hot_path = hot_dir.join(&hot_name);

            std::fs::copy(&artifact, &hot_path)?;

            let current = hot_dir.join(".current");
            if current.exists() {
                std::fs::remove_file(&current)?;
            }

            #[cfg(unix)]
            std::os::unix::fs::symlink(&hot_path, &current)?;

            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&hot_path, &current)?;

            if self.options.verbose {
                println!("Hot reload ready: {hot_name}");
            }
        }

        Ok(())
    }
}
