// linter-skip
//! Tests for TypeScript project cases harness
//!
//! This harness runs the JSON-driven project specs from tests/typescript/tests/cases/project/
//! by compiling multi-file projects and executing the emitted JS in Quench with a CommonJS loader.
// linter-skip
#![allow(unknown_lints, clippy::function_length, clippy::complexity, renamed_and_removed_lints, function_length, complexity, clippy::too_many_lines)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// Project spec parsed from JSON
#[derive(Debug, Deserialize)]
struct ProjectSpec {
    scenario: String,
    #[serde(rename = "projectRoot")]
    project_root: String,
    #[serde(rename = "inputFiles")]
    input_files: Vec<String>,
    #[serde(rename = "baselineCheck")]
    baseline_check: Option<bool>,
    #[serde(rename = "runTest")]
    run_test: Option<bool>,
    #[serde(rename = "declaration")]
    declaration: Option<bool>,
    #[serde(rename = "declarationDir")]
    declaration_dir: Option<String>,
}

/// Test result for a project spec
#[derive(Debug)]
enum ProjectResult {
    Pass,
    Skip(String),
    Fail(String),
}

/// Get the TypeScript test directory path
fn get_ts_dir() -> PathBuf {
    // Navigate from crates/quench-runtime/ to the repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")  // crates/quench-runtime -> crates/
        .join("..")  // crates -> repo root
        .join("tests")
        .join("typescript")
}

/// Find all project spec JSON files
fn find_project_specs() -> Vec<PathBuf> {
    let ts_dir = get_ts_dir();
    let project_dir = ts_dir.join("tests").join("cases").join("project");
    
    if !project_dir.exists() {
        return Vec::new();
    }
    
    let mut specs = Vec::new();
    if let Ok(entries) = fs::read_dir(&project_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                specs.push(path);
            }
        }
    }
    specs.sort();
    specs
}

/// Parse a project spec JSON file
fn parse_spec(path: &Path) -> Option<ProjectSpec> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Check if a project spec should be skipped
fn should_skip_spec(spec: &ProjectSpec) -> Option<String> {
    // Skip specs that don't require runtime execution
    if spec.run_test != Some(true) && spec.baseline_check != Some(true) {
        return Some("No runtime execution required".to_string());
    }
    
    // Skip declaration-only specs (no JS emit)
    if spec.declaration == Some(true) && spec.baseline_check != Some(true) && spec.run_test != Some(true) {
        return Some("Declaration-only spec".to_string());
    }
    
    // Skip AMD/System module specs
    if spec.project_root.contains("System") || spec.project_root.contains("AMD") {
        return Some("AMD/System module not supported".to_string());
    }
    
    None
}

/// Compile a project using TypeScript API via Node.js
/// Returns a map of emitted filename -> source content
fn compile_project(ts_dir: &Path, spec: &ProjectSpec) -> Result<HashMap<String, String>, String> {
    let project_path = ts_dir.join(&spec.project_root);
    
    // Create a Node.js script to compile the project
    let js_api_script = format!(r#"
const ts = require('{}');
const path = require('path');

// Get the input files with full paths
const inputFiles = [{}];
const options = ts.getDefaultCompilerOptions();
{}

const program = ts.createProgram(inputFiles, options);
const emitResult = program.emit();

// Collect emitted JS files
const emittedFiles = {{}};
program.getSourceFiles()
    .filter(f => !f.fileName.includes('node_modules'))
    .forEach(sourceFile => {{
        const baseName = path.basename(sourceFile.fileName);
        const jsName = baseName.replace(/\.tsx?$/, '.js');
        emittedFiles[jsName] = sourceFile.getFullText();
    }});

console.log(JSON.stringify(emittedFiles));
"#,
        ts_dir.join("lib").join("typescript.js").to_string_lossy(),
        spec.input_files.iter()
            .map(|f| format!("'{}'", project_path.join(f).to_string_lossy().replace("\\", "\\\\")))
            .collect::<Vec<_>>().join(", "),
        if let Some(ref decl_dir) = spec.declaration_dir {
            format!("options.declarationDir = '{}';", decl_dir)
        } else {
            String::new()
        }
    );
    
    // Run the compilation
    let output = Command::new("node")
        .arg("-e")
        .arg(&js_api_script)
        .output()
        .map_err(|e| format!("Failed to run Node.js: {}", e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Compilation failed: {}", stderr));
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .map_err(|e| format!("Failed to parse emitted files: {}", e))
}

/// Execute emitted JavaScript in quench-runtime with a basic CommonJS loader
fn execute_in_runtime(emitted_files: HashMap<String, String>) -> Result<(), String> {
    use quench_runtime::{Context, Value, Object, ObjectKind, NativeFunction};
    use std::rc::Rc;
    use std::cell::RefCell;
    
    let mut ctx = Context::new()
        .map_err(|e| format!("Failed to create context: {}", e))?;
    
    // Create a module registry for CommonJS require()
    let modules: Rc<RefCell<HashMap<String, Value>>> = Rc::new(RefCell::new(HashMap::new()));
    let modules_for_closure = Rc::clone(&modules);
    let emitted_clone = emitted_files.clone();
    
    // Create require() function
    let require_func = NativeFunction::new(move |args: Vec<Value>| {
        let module_name = args.first()
            .and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| quench_runtime::JsError("require() requires a string argument".to_string()))?;
        
        // Check if already loaded
        {
            let mods = modules_for_closure.borrow();
            if let Some(exports) = mods.get(&module_name) {
                return Ok(exports.clone());
            }
        }
        
        // Find the module in emitted files
        let key = module_name.trim_start_matches("./").trim_start_matches("../");
        let js_name = if key.ends_with(".js") {
            key.to_string()
        } else {
            format!("{}.js", key)
        };
        
        let content = emitted_clone.get(&js_name)
            .ok_or_else(|| quench_runtime::JsError(format!("Cannot find module: {}", module_name)))?
            .clone();
        
        // Create a new context for the module
        let mut mod_ctx = Context::new()
            .map_err(|e| quench_runtime::JsError(format!("Module context error: {}", e)))?;
        
        // Create module.exports
        let exports_obj = Object::new(ObjectKind::Ordinary);
        let exports_rc = Rc::new(RefCell::new(exports_obj));
        
        let module_obj = Object::new(ObjectKind::Ordinary);
        let module_ref = Rc::new(RefCell::new(module_obj));
        module_ref.borrow_mut().set("exports", Value::Object(Rc::clone(&exports_rc)));
        
        mod_ctx.set_global("exports".to_string(), Value::Object(Rc::clone(&exports_rc)));
        mod_ctx.set_global("module".to_string(), Value::Object(Rc::clone(&module_ref)));
        
        // Execute the module code
        mod_ctx.eval(&content)
            .map_err(|e| quench_runtime::JsError(format!("Module error: {}", e)))?;
        
        // Get the exports
        let exports = module_ref.borrow().get("exports")
            .unwrap_or(Value::Undefined);
        
        // Cache the module
        {
            let mut mods = modules_for_closure.borrow_mut();
            mods.insert(module_name, exports.clone());
        }
        
        Ok(exports)
    });
    
    ctx.set_global("require".to_string(), Value::NativeFunction(Rc::new(require_func)));
    
    // Execute each emitted file
    for (name, content) in emitted_files {
        ctx.eval(&content)
            .map_err(|e| format!("Error in {}: {}", name, e))?;
    }
    
    Ok(())
}

/// Run a single project spec
fn run_spec(path: &Path) -> ProjectResult {
    let spec = match parse_spec(path) {
        Some(s) => s,
        None => return ProjectResult::Fail(format!("Failed to parse {}", path.display())),
    };
    
    if let Some(reason) = should_skip_spec(&spec) {
        return ProjectResult::Skip(reason);
    }
    
    let ts_dir = get_ts_dir();
    
    // Compile the project
    let emitted_files = match compile_project(&ts_dir, &spec) {
        Ok(files) => files,
        Err(e) => return ProjectResult::Fail(format!("Compilation failed: {}", e)),
    };
    
    // Execute in runtime (pass by value since closure needs ownership)
    if let Err(e) = execute_in_runtime(emitted_files) {
        return ProjectResult::Fail(format!("Execution failed: {}", e));
    }
    
    ProjectResult::Pass
}

// =============================================================================
// Unit tests
// =============================================================================

#[test]
fn test_project_spec_discovery() {
    let specs = find_project_specs();
    assert!(!specs.is_empty(), "Should find project specs");
}

#[test]
fn test_parse_baseline_spec() {
    let path = get_ts_dir()
        .join("tests")
        .join("cases")
        .join("project")
        .join("baseline.json");
    
    let spec = parse_spec(&path).expect("Should parse baseline.json");
    assert_eq!(spec.scenario, "baseline");
    assert!(spec.baseline_check == Some(true) || spec.run_test == Some(true));
}

#[test]
fn test_should_skip_non_runtime_specs() {
    // circularReferencing.json has no runTest or baselineCheck
    let path = get_ts_dir()
        .join("tests")
        .join("cases")
        .join("project")
        .join("circularReferencing.json");
    
    let spec = parse_spec(&path).expect("Should parse");
    let reason = should_skip_spec(&spec);
    assert!(reason.is_some(), "Should be skipped");
}

#[test]
fn test_baseline_specs_not_skipped() {
    let path = get_ts_dir()
        .join("tests")
        .join("cases")
        .join("project")
        .join("baseline.json");
    
    let spec = parse_spec(&path).expect("Should parse");
    let reason = should_skip_spec(&spec);
    assert!(reason.is_none(), "Should not be skipped: {:?}", reason);
}

#[test]
fn test_commonjs_module_execution() {
    use quench_runtime::{Context, Value, Object, ObjectKind, NativeFunction};
    use std::rc::Rc;
    use std::cell::RefCell;
    
    let mut ctx = Context::new().unwrap();
    
    // Create a simple module.exports = { foo: 42 }
    let exports = Object::new(ObjectKind::Ordinary);
    let exports_rc = Rc::new(RefCell::new(exports));
    exports_rc.borrow_mut().set("foo", Value::Number(42.0));
    
    // Create require function that returns our module
    let exports_for_closure = Rc::clone(&exports_rc);
    let require_func = NativeFunction::new(move |_args: Vec<Value>| {
        Ok(Value::Object(Rc::clone(&exports_for_closure)))
    });
    
    ctx.set_global("require".to_string(), Value::NativeFunction(Rc::new(require_func)));
    
    // Test requiring a module
    let result = ctx.eval("var m = require('./test'); m.foo");
    assert!(result.is_ok(), "Should execute require: {:?}", result);
    if let Ok(Value::Number(n)) = result {
        assert_eq!(n, 42.0);
    } else {
        panic!("Expected number 42.0, got {:?}", result);
    }
}

#[test]
fn test_spec_count_by_category() {
    let specs = find_project_specs();
    let mut runnable = 0;
    let mut skipped = 0;
    
    for path in &specs {
        if let Some(spec) = parse_spec(path) {
            if should_skip_spec(&spec).is_some() {
                skipped += 1;
            } else {
                runnable += 1;
            }
        }
    }
    
    println!("Runnable: {}, Skipped: {}, Total: {}", runnable, skipped, specs.len());
    assert!(runnable > 0, "Should have at least some runnable specs");
}

#[test]
#[ignore = "requires Node.js and TypeScript compilation"]
fn test_run_baseline_project() {
    let path = get_ts_dir()
        .join("tests")
        .join("cases")
        .join("project")
        .join("baseline.json");
    
    let result = run_spec(&path);
    match result {
        ProjectResult::Pass => {},
        ProjectResult::Skip(reason) => {
            eprintln!("Skipped: {}", reason);
        }
        ProjectResult::Fail(msg) => {
            panic!("Baseline test failed: {}", msg);
        }
    }
}
