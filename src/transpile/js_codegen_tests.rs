#[cfg(test)]
mod tests {
    use super::super::js_codegen::generate_island_js;
    use super::super::hir::*;

    fn parse_island(source: &str) -> Module {
        let parser = crate::transpile::TsParser::new();
        parser.parse_tsx(source).unwrap()
    }

    #[test]
    fn test_generate_counter_island() {
        let source = r#"
import { useState } from "preact/hooks";

interface Props {
  initial?: number;
}

export default function Counter({ initial = 0 }: Props) {
  const [count, setCount] = useState(initial);
  return (
    <div>
      <button onClick={() => setCount(count + 1)}>+</button>
      <span>{count}</span>
    </div>
  );
}
"#;
        let module = parse_island(source);
        let js = generate_island_js("Counter", &module);
        // Check that JS was generated
        assert!(!js.is_empty(), "Expected JS output");
        assert!(js.contains("function Counter") || js.contains("Counter"), "Expected Counter function");
    }

    #[test]
    fn test_generate_simple_island() {
        let source = r#"
export default function Hello({ name = "World" }) {
  return <h1>Hello, {name}!</h1>;
}
"#;
        let module = parse_island(source);
        let js = generate_island_js("Hello", &module);
        // Check that JS was generated
        assert!(!js.is_empty(), "Expected JS output");
        assert!(js.contains("function Hello") || js.contains("Hello"), "Expected Hello function");
    }
}
