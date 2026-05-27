//! Initialize a new runts project

use anyhow::{Result, Context};
use std::path::PathBuf;
use tracing::info;

pub async fn run_init(name: String, path: Option<PathBuf>) -> Result<()> {
    let project_dir = if let Some(p) = path {
        p.join(&name)
    } else {
        PathBuf::from(&name)
    };

    info!("Creating new runts project: {}", name);
    info!("Location: {:?}", project_dir);

    // User project: TS/TSX only — no Rust source lives here
    let dirs = ["routes", "islands", "components", "static"];
    for dir in dirs {
        let path = project_dir.join(dir);
        std::fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create: {}", path.display()))?;
    }

    let index_tsx = r#"export default function Index() {
    return <h1>Hello from runts!</h1>;
}
"#;
    std::fs::write(project_dir.join("routes/index.tsx"), index_tsx)?;

    let counter_tsx = r#"import { useState } from "preact/hooks";

interface CounterProps {
  initial?: number;
}

export default function Counter({ initial = 0 }: CounterProps) {
    const [count, setCount] = useState(initial);
    return (
        <div>
            <p>Count: {count}</p>
            <button onClick={() => setCount(count + 1)}>Increment</button>
        </div>
    );
}
"#;
    std::fs::write(project_dir.join("islands/Counter.tsx"), counter_tsx)?;

    let header_tsx = r#"export function Header({ title }: { title: string }) {
    return <header><h1>{title}</h1></header>;
}
"#;
    std::fs::write(project_dir.join("components/Header.tsx"), header_tsx)?;

    let tsconfig = r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "jsx": "react-jsx",
    "jsxImportSource": "preact",
    "strict": true
  },
  "include": ["routes/**", "islands/**", "components/**"]
}
"#;
    std::fs::write(project_dir.join("tsconfig.json"), tsconfig)?;

    info!("Project created!");
    Ok(())
}
