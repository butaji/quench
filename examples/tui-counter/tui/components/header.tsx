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

interface HeaderProps {
  title: string;
}

export default function Header({ title }: HeaderProps) {
  return (
    <Block title={title} borders={true}>
      <Text>Press 'q' to quit</Text>
    </Block>
  );
}
