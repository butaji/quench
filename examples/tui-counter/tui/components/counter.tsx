// Intrinsic elements for Ratatui compilation
declare global {
  namespace JSX {
    interface IntrinsicElements {
      Block: { title?: string; borders?: boolean; children?: any };
      Layout: { direction?: "vertical" | "horizontal"; children?: any };
      Text: { children?: string };
    }
  }
}

interface CounterProps {
  initial?: number;
}

export default function Counter({ initial = 0 }: CounterProps) {
  // For v0.1, the event handling will be wired by the compiler
  // The user writes TSX-like components, compiler generates Ratatui widgets + event loop
  return (
    <Block title="Counter" borders={true}>
      <Layout direction="vertical">
        <Text>Count: {initial}</Text>
        <Text>Press ↑/↓ to change</Text>
      </Layout>
    </Block>
  );
}
