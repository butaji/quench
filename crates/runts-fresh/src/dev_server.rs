//! Dev server management for Fresh plugin

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use runts_plugin::{DevState, PluginError};

/// Dev state for Fresh plugin - tracks server process
pub struct FreshDevState {
    /// Project root directory
    project_root: PathBuf,
    /// Whether server has been spawned
    spawned: Arc<Mutex<bool>>,
    /// Child process handle (None until spawned)
    child: Arc<Mutex<Option<std::process::Child>>>,
}

impl DevState for FreshDevState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl FreshDevState {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            spawned: Arc::new(Mutex::new(false)),
            child: Arc::new(Mutex::new(None)),
        }
    }

    /// Spawn the dev server if not already spawned
    pub fn ensure_server_running(&self) -> Result<(), PluginError> {
        if self.check_server_running() {
            return Ok(());
        }
        self.spawn_server()
    }

    fn check_server_running(&self) -> bool {
        let spawned = self.spawned.lock().unwrap();
        if !*spawned {
            return false;
        }
        drop(spawned);
        let mut child_guard = self.child.lock().unwrap();
        if let Some(ref mut child) = *child_guard {
            Self::is_child_running(child)
        } else {
            false
        }
    }

    fn is_child_running(child: &mut std::process::Child) -> bool {
        match child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    fn spawn_server(&self) -> Result<(), PluginError> {
        let binary_path = self.get_binary_path()?;
        if !binary_path.exists() {
            self.compile_project()?;
        }
        self.start_server_process(&binary_path)
    }

    fn get_binary_path(&self) -> Result<PathBuf, PluginError> {
        let build_dir = self.project_root.join(".runts").join("build");
        Ok(build_dir.join("target").join("debug").join("runts-app"))
    }

    fn start_server_process(&self, binary_path: &std::path::Path) -> Result<(), PluginError> {
        println!("Starting dev server at http://127.0.0.1:8000");
        println!("Note: Hot reload coming in v0.2 - restart server manually for now");

        let child = Command::new(binary_path)
            .current_dir(&self.project_root)
            .spawn()
            .map_err(|e| PluginError::new("fresh", "", &format!("failed to start server: {}", e)))?;

        let mut spawned = self.spawned.lock().unwrap();
        *spawned = true;
        let mut child_guard = self.child.lock().unwrap();
        *child_guard = Some(child);

        Ok(())
    }

    fn compile_project(&self) -> Result<(), PluginError> {
        self.compile_project_with_modules(0)
    }

    fn compile_project_with_modules(&self, module_count: usize) -> Result<(), PluginError> {
        let build_dir = self.project_root.join(".runts").join("build");

        if !build_dir.exists() {
            return Err(PluginError::new("fresh", "", "runts build directory not found. Run 'runts build' first."));
        }

        if module_count > 0 {
            println!("Compiling {} modules...", module_count);
        } else {
            println!("Compiling...");
        }
        let output = Command::new("cargo")
            .current_dir(&build_dir)
            .args(&["build"])
            .output()
            .map_err(|e| PluginError::new("fresh", "", &format!("cargo build failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PluginError::new("fresh", "", &format!("cargo build failed:\n{}", stderr)));
        }

        Ok(())
    }

    /// Recompile after file change
    pub fn recompile(&self, module_count: usize) -> Result<(), PluginError> {
        // Kill old server
        {
            let mut spawned = self.spawned.lock().unwrap();
            let mut child_guard = self.child.lock().unwrap();
            if let Some(ref mut child) = *child_guard {
                let _ = child.kill();
            }
            *spawned = false;
            *child_guard = None;
        }

        // Recompile
        self.compile_project_with_modules(module_count)?;

        // Restart server
        self.ensure_server_running()
    }
}
