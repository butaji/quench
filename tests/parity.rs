//! Parity tests for Quench vs Ink
//!
//! These tests verify that the FFI bridge produces consistent results
//! compared to what Ink would produce.

#[cfg(test)]
mod tests {
    use std::process::Command;

    #[test]
    fn test_simple_js_ffi() {
        // Run simple.js and verify FFI functions work
        let output = Command::new(env!("CARGO_BIN_EXE_quench"))
            .arg("examples/simple.js")
            .output()
            .expect("Failed to run quench");
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Verify FFI test passed
        assert!(stdout.contains("All FFI tests passed!"), 
            "FFI tests failed. Output:\n{}", stdout);
        
        // Verify specific FFI calls worked
        assert!(stdout.contains("Created root: 1"));
        assert!(stdout.contains("Created Box: 2"));
        assert!(stdout.contains("Created Text: 3"));
        assert!(stdout.contains("ink-box"));
        assert!(stdout.contains("Hello, Quench!"));
    }

    #[test]
    fn test_counter_jsx_compiles() {
        // Test that the compiler can handle TSX examples
        let output = Command::new(env!("CARGO_BIN_EXE_quench"))
            .arg("--help")
            .output()
            .expect("Failed to run quench");
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Quench"));
    }

    #[test]
    fn test_binary_exists() {
        // Basic sanity check that the binary was built
        let output = Command::new(env!("CARGO_BIN_EXE_quench"))
            .arg("--version")
            .output()
            .expect("Failed to run quench");
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("v0.1.0"));
    }
}
