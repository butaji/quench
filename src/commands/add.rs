//! Add new components, routes, or islands
//!
//! Generates starter files for:
//! - Islands (interactive components)
//! - Components (server-only)
//! - Routes (pages)
//! - Middleware

use crate::util::to_pascal_case;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

/// Re-export ComponentType from cli module
pub use crate::cli::ComponentType;

/// Run the add command
pub async fn run_add(
    component_type: ComponentType,
    name: String,
    path: Option<PathBuf>,
) -> Result<()> {
    let project_root = find_project_root(path.as_ref())?;
    info!(
        "Adding {} '{}' to {:?}",
        component_type.as_str(),
        name,
        project_root
    );
    add_component_type(component_type, &project_root, &name)?;
    info!(
        "Successfully created {} '{}'",
        component_type.as_str(),
        name
    );
    Ok(())
}

fn add_component_type(
    component_type: ComponentType,
    project_root: &Path,
    name: &str,
) -> Result<()> {
    match component_type {
        ComponentType::Island => add_island(&project_root.to_path_buf(), name),
        ComponentType::Component => add_component(&project_root.to_path_buf(), name),
        ComponentType::Route => add_route(&project_root.to_path_buf(), name),
        ComponentType::Middleware => add_middleware(&project_root.to_path_buf(), name),
    }
}

/// Find project root
fn find_project_root(path: Option<&PathBuf>) -> Result<PathBuf> {
    let start = path.cloned().unwrap_or_else(|| PathBuf::from("."));

    let mut current = start.clone();
    loop {
        if current.join("Cargo.toml").exists() || current.join("runts.config.ts").exists() {
            return Ok(current);
        }

        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            anyhow::bail!("Not a runts project (no Cargo.toml or runts.config.ts found)");
        }
    }
}

// Import the helper functions from the generated code module
mod generated {
    use super::*;

    /// Add an island component
    pub fn add_island(project_root: &std::path::Path, name: &str) -> Result<()> {
        let islands_dir = project_root.join("islands");
        std::fs::create_dir_all(&islands_dir).context("Failed to create islands directory")?;

        let pascal_name = to_pascal_case(name);
        let content = generate_island_code(&pascal_name);
        let file_path = islands_dir.join(format!("{}.tsx", pascal_name));

        if file_path.exists() {
            anyhow::bail!("Island '{}' already exists at {:?}", pascal_name, file_path);
        }

        std::fs::write(&file_path, content).context("Failed to write island file")?;

        info!("  Created: islands/{}.tsx", pascal_name);
        Ok(())
    }

    /// Add a server component
    pub fn add_component(project_root: &std::path::Path, name: &str) -> Result<()> {
        let components_dir = project_root.join("components");
        std::fs::create_dir_all(&components_dir)
            .context("Failed to create components directory")?;

        let pascal_name = to_pascal_case(name);
        let content = generate_component_code(&pascal_name);
        let file_path = components_dir.join(format!("{}.tsx", pascal_name));

        if file_path.exists() {
            anyhow::bail!(
                "Component '{}' already exists at {:?}",
                pascal_name,
                file_path
            );
        }

        std::fs::write(&file_path, content).context("Failed to write component file")?;

        info!("  Created: components/{}.tsx", pascal_name);
        Ok(())
    }

    /// Add a route
    pub fn add_route(project_root: &PathBuf, name: &str) -> Result<()> {
        let routes_dir = project_root.join("routes");
        std::fs::create_dir_all(&routes_dir).context("Failed to create routes directory")?;
        let file_path = resolve_route_path(&routes_dir, name)?;
        if file_path.exists() {
            anyhow::bail!("Route '{}' already exists at {:?}", name, file_path);
        }
        std::fs::write(&file_path, generate_route_code(name))
            .context("Failed to write route file")?;
        info!("  Created: routes/{}.tsx", name);
        Ok(())
    }

    fn resolve_route_path(routes_dir: &Path, name: &str) -> Result<PathBuf> {
        let parts: Vec<&str> = name.split('/').collect();
        let mut file_path = routes_dir.to_path_buf();
        for part in &parts {
            file_path = file_path.join(part);
        }
        let final_path = if file_path.to_string_lossy().contains('[')
            || file_path.to_string_lossy().ends_with("index")
        {
            PathBuf::from(format!("{}.tsx", file_path.to_string_lossy()))
        } else {
            file_path.with_file_name(format!("{}.tsx", parts.last().unwrap_or(&name)))
        };
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(final_path)
    }

    /// Add middleware
    pub fn add_middleware(project_root: &PathBuf, name: &str) -> Result<()> {
        let routes_dir = project_root.join("routes");

        let middleware_path = if name.contains('/') {
            routes_dir
                .join(name)
                .parent()
                .unwrap_or(&routes_dir)
                .join("_middleware.ts")
        } else {
            routes_dir.join(format!("{}/_middleware.ts", name))
        };

        if let Some(parent) = middleware_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create middleware directory")?;
        }

        if middleware_path.exists() {
            anyhow::bail!("Middleware already exists at {:?}", middleware_path);
        }

        let content = generate_middleware_code(name);
        std::fs::write(&middleware_path, content).context("Failed to write middleware file")?;

        info!("  Created: routes/{}/_middleware.ts", name);
        Ok(())
    }

    fn generate_island_code(name: &str) -> String {
        let props_type = format!("{}Props", name);
        format!(
            r#"import {{ useState, useEffect }} from "preact/hooks";
import {{ IS_BROWSER }} from "fresh/runtime";

interface {} {{
  initial?: number;
}}

export default function {}({{ initial = 0 }}: {}) {{
  const [count, setCount] = useState(initial);

  useEffect(() => {{
    if (IS_BROWSER) {{
      console.log("{} mounted on client");
    }}
  }}, []);

  return (
    <div class="{}">
      <p>Count: {{count}}</p>
      <button onClick={{() => setCount(count + 1)}}>
        Increment
      </button>
    </div>
  );
}}
"#,
            props_type, name, props_type, name, name
        )
    }

    fn generate_component_code(name: &str) -> String {
        let props_type = format!("{}Props", name);
        format!(
            r#"interface {} {{
  title: string;
  children?: any;
}}

export function {}({{ title, children }}: {}) {{
  return (
    <div class="{}">
      <h2>{{title}}</h2>
      {{children}}
    </div>
  );
}}
"#,
            props_type, name, props_type, name
        )
    }

    fn generate_route_code(name: &str) -> String {
        let route_name = name.replace('/', "-");
        let pascal_name = to_pascal_case(&route_name);

        format!(
            r#"export default function {}() {{
  return (
    <main>
      <h1>{{"{}"}}</h1>
      <a href="/">Back home</a>
    </main>
  );
}}
"#,
            pascal_name, route_name
        )
    }

    fn generate_middleware_code(name: &str) -> String {
        format!(
            r#"export default async function middleware(ctx) {{
  console.log("Middleware '{}' - path:", ctx.url.pathname);
  return await ctx.next();
}}
"#,
            name
        )
    }
}

pub use generated::*;
