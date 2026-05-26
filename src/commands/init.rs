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

    let dirs = ["routes", "islands", "components", "static", "src"];
    for dir in dirs {
        let path = project_dir.join(dir);
        std::fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create: {}", path.display()))?;
    }

    let main_rs = r#"use std::net::SocketAddr;
use axum::{Router, routing::get, response::Html};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();
    let app = Router::new().route("/", get(handler));
    let addr = SocketAddr::from(([127,0,0,1], 8000));
    tracing::info!("Listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handler() -> Html<&'static str> {
    Html("<html><body><h1>Hello runts!</h1></body></html>")
}
"#;
    
    std::fs::write(project_dir.join("src/main.rs"), main_rs)
        .context("Failed to write main.rs")?;

    let cargo = format!(r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[[bin]]
name = "{}"
path = "src/main.rs"

[dependencies]
runts_lib = {{ path = "../runie-tsx" }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
tokio = {{ version = "1.0", features = ["full"] }}
axum = "0.7"
tower = "0.4"
tower-http = {{ version = "0.5", features = ["fs", "cors"] }}
hyper = {{ version = "1.3", features = ["server", "http1"] }}
chrono = {{ version = "0.4", features = ["serde"] }}
clap = {{ version = "4.5", features = ["derive", "env"] }}
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}

[profile.release]
lto = true
codegen-units = 1
"#, 
        name.replace("-", "_"),
        name.replace("-", "_")
    );
    
    std::fs::write(project_dir.join("Cargo.toml"), cargo)
        .context("Failed to write Cargo.toml")?;

    let lib_rs = "pub mod routes;\npub mod islands;\n";
    std::fs::write(project_dir.join("src/lib.rs"), lib_rs)?;

    let index_tsx = r#"export default function Index() {
    return <h1>Hello from runts!</h1>;
}
"#;
    std::fs::write(project_dir.join("routes/index.tsx"), index_tsx)?;

    let counter_tsx = r#"import { useState } from "preact/hooks";
export default function Counter({ initial = 0 }) {
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
