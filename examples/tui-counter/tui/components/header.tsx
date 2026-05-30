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
