use quench_runtime::Context;
use std::path::Path;

fn main() {
    // First, load runtime.js
    let runtime_path = Path::new("src/runtime.js");
    let source = if runtime_path.exists() {
        std::fs::read_to_string(runtime_path).unwrap()
    } else {
        // Try relative to workspace root
        std::fs::read_to_string("../../../src/runtime.js").unwrap()
    };
    println!("runtime.js size: {} bytes", source.len());
    
    let mut ctx = Context::new().expect("Failed to create context");
    
    // Try to evaluate runtime.js step by step
    println!("Loading runtime.js...");
    match ctx.eval(&source) {
        Ok(v) => println!("runtime.js loaded successfully, last value: {:?}", v),
        Err(e) => println!("Error loading runtime.js: {:?}", e),
    }
    
    // Now try the counter example
    println!("\nTrying counter.js...");
    let counter_path = Path::new("examples/counter.js");
    let counter_source = if counter_path.exists() {
        std::fs::read_to_string(counter_path).unwrap()
    } else {
        std::fs::read_to_string("../../../examples/counter.js").unwrap()
    };
    println!("counter.js size: {} bytes", counter_source.len());
    
    match ctx.eval(&counter_source) {
        Ok(v) => println!("counter.js result: {:?}", v),
        Err(e) => println!("Error in counter.js: {:?}", e),
    }
}
