//! Initialize a new runts project

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

pub async fn run_init(name: String, path: Option<PathBuf>) -> Result<()> {
    let project_dir = path
        .map(|p| p.join(&name))
        .unwrap_or_else(|| PathBuf::from(&name));
    info!("Creating new runts project: {}", name);
    info!("Location: {:?}", project_dir);
    create_template_files(&project_dir)?;
    info!("Project created!");
    Ok(())
}

fn create_template_files(project_dir: &Path) -> Result<()> {
    for dir in &["routes", "islands", "components", "static"] {
        std::fs::create_dir_all(project_dir.join(dir))
            .with_context(|| format!("Failed to create: {}", dir))?;
    }
    std::fs::write(
        project_dir.join("routes/index.tsx"),
        r#"export default function Index() { return <h1>Hello from runts!</h1>; }"#,
    )?;
    std::fs::write(
        project_dir.join("islands/Counter.tsx"),
        r#"import { useState } from "preact/hooks"; export default function Counter({ initial = 0 }: { initial?: number }) { const [count, setCount] = useState(initial); return <div><p>Count: {count}</p><button onClick={() => setCount(count + 1)}>+</button></div>; }"#,
    )?;
    std::fs::write(
        project_dir.join("components/Header.tsx"),
        r#"export function Header({ title }: { title: string }) { return <header><h1>{title}</h1></header>; }"#,
    )?;
    let tsconfig = serde_json::json!({
        "compilerOptions": {
            "target": "ESNext",
            "module": "ESNext",
            "jsx": "react-jsx",
            "jsxImportSource": "preact",
            "strict": true
        },
        "include": ["routes/**", "islands/**", "components/**"]
    });
    std::fs::write(project_dir.join("tsconfig.json"), serde_json::to_string_pretty(&tsconfig)?)?;
    Ok(())
}
