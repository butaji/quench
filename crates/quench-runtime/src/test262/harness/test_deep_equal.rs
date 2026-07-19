//! Unit tests for assert.deepEqual behavior

#[cfg(test)]
mod tests {
    use crate::test262::QuenchHost;

    #[test]
    fn test_native_function_property_set_and_get() {
        // This tests that NativeFunction.set_property and member access work correctly
        let mut host = QuenchHost::new();
        
        // Run a script that creates a native function with properties and verifies them
        let result = host.run_script(r#"
            // Verify we can set and get properties on functions
            function testFn() { return 42; }
            testFn.prop1 = "value1";
            testFn.prop2 = function() { return "function"; };
            
            // Check the properties
            var p1 = testFn.prop1;
            var p2 = typeof testFn.prop2;
            
            // Return results
            if (p1 !== "value1") throw new Error("prop1 mismatch: " + p1);
            if (p2 !== "function") throw new Error("prop2 should be function");
            
            // Try calling the property
            if (testFn.prop2() !== "function") throw new Error("prop2() failed");
            
            "ok"
        "#);
        
        match result {
            Ok(v) => println!("Result: {:?}", v),
            Err(e) => panic!("Test failed: {:?}", e),
        }
    }
}
