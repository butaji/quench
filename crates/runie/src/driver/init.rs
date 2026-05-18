//! # Project Initialization
//!
//! Creates minimal runie files in existing Rust project.

use super::templates;
use crate::Result;

impl super::BuildDriver {
    /// Initialize runie in existing project.
    pub fn init_project_structure(&self) -> Result<()> {
        let project_name = &self.config.project.name;

        // Create main.r.tsx if it doesn't exist
        let main_file = self.options.workspace.join("src/main.r.tsx");
        if !main_file.exists() {
            std::fs::write(&main_file, templates::MAIN_RS)?;
            println!("Created: {}", main_file.display());
        }

        println!("Initialized runie for project: {}", project_name);
        println!("Run 'cargo runie dev' to start hot reload");
        Ok(())
    }
}
