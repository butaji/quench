//! # Project Initialization
//!
//! Creates new Rune projects with proper structure.
#![allow(clippy::literal_string_with_formatting_args)]

use super::templates;
use crate::Result;

/// Replace a placeholder in template string.
fn template_replace(template: &str, placeholder: &str, value: &str) -> String {
    template.replace(placeholder, value)
}

impl super::BuildDriver {
    /// Initialize project structure.
    pub fn init_project_structure(&self) -> Result<()> {
        let project_name = &self.config.project.name;
        let target_crate = &self.config.build.target_crate;
        let base = self.options.workspace.join("crates");

        // Create directory structure
        std::fs::create_dir_all(base.join(target_crate).join("src/native"))?;
        std::fs::create_dir_all(base.join(target_crate).join("src/views"))?;
        std::fs::create_dir_all(base.join("protocol/src"))?;
        std::fs::create_dir_all(base.join("host/src"))?;

        // Create workspace Cargo.toml
        let workspace_cargo = template_replace(
            &template_replace(templates::WORKSPACE_CARGO, "{target_crate}", target_crate),
            "{authors}",
            project_name,
        );
        std::fs::write(self.options.workspace.join("Cargo.toml"), workspace_cargo)?;

        // Create rune.toml
        let rune_config = template_replace(
            &template_replace(templates::RUNE_CONFIG, "{name}", project_name),
            "{target_crate}",
            target_crate,
        );
        std::fs::write(self.options.workspace.join("rune.toml"), rune_config)?;

        Ok(())
    }

    /// Initialize protocol crate.
    pub fn init_protocol(&self) -> Result<()> {
        let proto_dir = self.options.workspace.join("crates/protocol");

        std::fs::write(proto_dir.join("Cargo.toml"), templates::PROTOCOL_CARGO)?;
        std::fs::write(proto_dir.join("src/lib.rs"), templates::PROTOCOL_LIB)?;

        Ok(())
    }

    /// Initialize host crate.
    pub fn init_host(&self) -> Result<()> {
        let host_dir = self.options.workspace.join("crates/host");
        let host_name = &self.config.build.host_crate;

        let cargo = template_replace(templates::HOST_CARGO, "{name}", host_name);
        std::fs::write(host_dir.join("Cargo.toml"), cargo)?;
        std::fs::write(host_dir.join("src/main.rs"), templates::HOST_MAIN)?;

        Ok(())
    }

    /// Initialize app crate.
    pub fn init_app(&self) -> Result<()> {
        let app_dir = self
            .options
            .workspace
            .join("crates")
            .join(&self.config.build.target_crate);
        let app_name = &self.config.build.target_crate;

        // Cargo.toml
        let cargo = template_replace(templates::APP_CARGO, "{name}", app_name);
        std::fs::write(app_dir.join("Cargo.toml"), cargo)?;

        // lib.rs
        std::fs::write(app_dir.join("src/lib.rs"), templates::APP_LIB)?;

        // Native module
        std::fs::create_dir_all(app_dir.join("src/native"))?;
        std::fs::write(app_dir.join("src/native/mod.rs"), templates::NATIVE_MOD)?;
        std::fs::write(
            app_dir.join("src/native/fast_math.rs"),
            templates::FAST_MATH,
        )?;

        // Main Rune file
        std::fs::write(app_dir.join("src/main.r.ts"), templates::MAIN_RS)?;

        // State Rune file
        std::fs::write(app_dir.join("src/state.r.ts"), templates::STATE_RS)?;

        // Root view (TSX)
        std::fs::create_dir_all(app_dir.join("src/views"))?;
        std::fs::write(app_dir.join("src/views/root.r.tsx"), templates::ROOT_RSX)?;
        std::fs::write(
            app_dir.join("src/views/task_list.r.tsx"),
            templates::TASK_LIST_RSX,
        )?;

        Ok(())
    }
}
