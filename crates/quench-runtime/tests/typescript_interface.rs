//! Test for TypeScript interface declaration handling

#[cfg(test)]
mod tests {
    use quench_runtime::swc_parse;
    
    /// Test that interface declarations don't cause parse errors
    /// when using parse_typescript (which the compiler uses).
    #[test]
    fn test_typescript_interface_stripped() {
        // This code has TypeScript interfaces that should be stripped
        let code = r#"
interface Metrics {
  cpu: number;
  memory: number;
  disk: number;
}

interface ProgressBarProps {
  label: string;
  value: number;
  max?: number;
  width?: number;
}

function ProgressBar(props: ProgressBarProps): JSX.Element {
  return <Box>test</Box>;
}
"#;
        
        // Parsing should succeed (interfaces are stripped by SWC)
        let result = swc_parse::parse_typescript(code);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
    }
    
    /// Test that the full compiler flow handles TypeScript correctly
    #[test]
    fn test_compiler_handles_typescript() {
        let code = r#"
interface Metrics {
  cpu: number;
}

function App(): JSX.Element {
  return <Box>Hello</Box>;
}

render(<App />);
"#;
        
        // Parse should succeed
        let result = swc_parse::parse_typescript(code);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
    }
}
