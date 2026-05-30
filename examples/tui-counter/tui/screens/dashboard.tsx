import Header from "../components/header.tsx";
import Counter from "../components/counter.tsx";

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

export default function Dashboard() {
  return (
    <Layout direction="vertical">
      <Header title="runts TUI Dashboard" />
      <Layout direction="horizontal">
        <Block title="Status">
          <Text>System: Online</Text>
        </Block>
        <Counter initial={0} />
      </Layout>
    </Layout>
  );
}
