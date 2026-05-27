#[cfg(test)]
mod tests {
    use super::super::js_codegen::generate_island_js;
    use super::super::hir::*;

    fn parse_island(source: &str) -> Module {
        let mut parser = crate::transpile::Parser::new();
        parser.parse_source(source).unwrap()
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
        assert!(js.contains("import { h, render } from 'preact';"));
        assert!(js.contains("import { useState } from 'preact/hooks';"));
        assert!(js.contains("export default function CounterComponent("));
        assert!(js.contains("useState"));
        assert!(js.contains("h('div'"));
        assert!(js.contains("h('button'"));
        assert!(js.contains("h('span'"));
        assert!(js.contains("Runts.registerIsland('Counter'"));
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
        assert!(js.contains("import { h, render } from 'preact';"));
        assert!(js.contains("export default function HelloComponent("));
        assert!(js.contains("h('h1'"));
        assert!(js.contains("Runts.registerIsland('Hello'"));
    }
}
