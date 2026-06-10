// Stdin/Stdout/Stderr Demo — TuiBridge
// Demonstrates useStdin, useStdout, useStderr hooks

import { render, Box, Text, useState, useStdin, useStdout, useStderr, useInput, useApp } from 'ink';

function StdinStdoutDemo(): JSX.Element {
  const stdin = useStdin();
  const stdout = useStdout();
  const stderr = useStderr();
  const [events, setEvents] = useState<string[]>([]);

  function addEvent(msg: string) {
    setEvents(e => {
      const next = e.slice(-9);
      next.push(msg);
      return next;
    });
  }

  useInput((input: string, key: { ctrl: boolean; shift: boolean }) => {
    if (input === 'q') { useApp().exit(); return; }
    if (input === 's') { stdout.write('Direct stdout: hello from useStdout\n'); addEvent('stdout.write()'); return; }
    if (input === 'e') { stderr.write('Direct stderr: error from useStderr\n'); addEvent('stderr.write()'); return; }
    addEvent(`key: ${input} (ctrl=${key.ctrl} shift=${key.shift})`);
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Stdin / Stdout / Stderr Hooks</Text>
      <Text dimColor>[s] stdout | [e] stderr | type for stdin | [q] quit</Text>
      <Text> </Text>
      <Box flexDirection="row">
        <Text color="cyan">RawMode: {String(stdin.isRawMode())}  </Text>
        <Text color="cyan">Columns: {stdout.columns}</Text>
      </Box>
      <Text> </Text>
      <Box flexDirection="column" borderStyle="single" padding={1} height={8}>
        <Text bold>Event Log:</Text>
        {events.length === 0
          ? <Text dimColor>(no events yet)</Text>
          : <Box flexDirection="column">{events.map((e, i) => <Text key={i}>{e}</Text>)}</Box>}
      </Box>
    </Box>
  );
}

render(<StdinStdoutDemo />);
