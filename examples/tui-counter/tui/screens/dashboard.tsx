import Header from "../components/header.tsx";
import Counter from "../components/counter.tsx";

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
