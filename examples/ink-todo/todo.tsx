// Ink-style JSX: Box, Text, Newline, Spacer.
// This file is the **source** the runts build pipeline
// parses. The `runts-ratatui` plugin (with Ink tags
// wired in this turn) walks the HIR and emits
// `runts_ink::Box` / `runts_ink::Text` / etc. into a
// Rust source tree, which `cargo build` then compiles
// to a native binary.

declare global {
  namespace JSX {
    interface IntrinsicElements {
      Box: {
        flexDirection?: "row" | "column";
        padding?: number;
        children?: any;
      };
      Text: {
        children?: string | number;
        bold?: boolean;
        color?: string;
      };
      Newline: Record<string, never>;
      Spacer: Record<string, never>;
    }
  }
}

interface Todo {
  done: boolean;
  text: string;
}

interface TodoProps {
  items: Todo[];
}

export default function TodoList({ items }: TodoProps) {
  const remaining = items.filter((t) => !t.done).length;
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Ink Todo</Text>
      <Newline />
      {items.map((t, i) => (
        <Text key={i}>{t.done ? "[x] " : "[ ] "}{t.text}</Text>
      ))}
      <Newline />
      <Text italic>{remaining} of {items.length} remaining</Text>
      <Spacer />
      <Text>Press q to quit.</Text>
    </Box>
  );
}
