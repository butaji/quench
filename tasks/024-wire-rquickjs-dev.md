# Task 024: Wire runts dev to Execute JS Bundle in rquickjs + Render to String

**Priority:** P0-Critical  
**Phase:** 1 — rquickjs Dev Engine  
**ETA:** 3–4 hours  
**Depends on:** 023

## The Problem

`runts dev` currently does nothing useful for the ratatui plugin. We need it to:
1. Watch `.tsx` files
2. Transpile changed files to JS
3. Create rquickjs context with bridge
4. Eval JS bundle
5. Capture render output

## Steps

### Step 1: Update `src/commands/dev/mod.rs`

Replace the no-op dev loop with:

```rust
pub async fn run_dev_server(config: &Config, path: PathBuf, plugin_name: String) -> Result<()> {
    let plugin = plugin::get_plugin(&plugin_name)?;
    let project_root = resolve_project_root(&path)?;
    let mut ctx = DevContext { root: project_root.clone(), modules: vec![] };

    // Initial render
    let output = render_project(&project_root).await?;
    println!("{}", output);

    let (_watcher, _tx, rx) = setup_file_watcher(&project_root)?;

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(event)) => {
                if should_reload(&event) {
                    let output = render_project(&project_root).await?;
                    println!("{}", output);
                }
            }
            // ...
        }
    }
}
```

### Step 2: Implement `render_project`

```rust
async fn render_project(project_root: &Path) -> Result<String> {
    let app_tsx = project_root.join("tui").join("app.tsx");
    let source = fs::read_to_string(&app_tsx)?;

    // Transpile
    let js = transpile_to_js(&source)?;

    // Setup rquickjs + bridge
    let rt = QuickJsRuntime::new();
    rt.register_ink_bridge()?;

    // Inject React shim
    rt.eval(REACT_SHIM)?;

    // Eval user bundle
    rt.eval(&js)?;

    // Call render and get output
    let output = rt.eval(r#"
        var tree = React.createElement(App, {});
        __runts_ink_bridge__.renderToString(tree);
    "#)?;

    Ok(output)
}
```

### Step 3: Add `--watch` / `--once` flags to `runts dev`

For parity harness we want `--once` (single render, exit):
```bash
runts dev --once --plugin ratatui examples/ink-text-props
```

### Step 4: Verify against deno

```bash
deno run -A examples/ink-text-props/main.tsx > deno.txt
runts dev --once --plugin ratatui examples/ink-text-props > rq.txt
diff deno.txt rq.txt
```

## Acceptance Criteria

- [ ] `runts dev --once --plugin ratatui examples/ink-text-props` prints the same text as deno.
- [ ] `runts dev --plugin ratatui examples/ink-text-props` watches files and re-renders on change.
- [ ] All static examples render without error.
