// Focus Form Example - TuiBridge demo (TypeScript)
// Demonstrates focus management and input handling

import { render, Box, Text, useState, useInput, useFocus, useFocusManager } from 'ink';

interface FormField {
  label: string;
  value: string;
}

function FocusForm(): JSX.Element {
  const [fields, setFields] = useState<FormField[]>([
    { label: 'Name:', value: 'Alice' },
    { label: 'Email:', value: 'alice@example.com' },
    { label: 'Password:', value: '***' },
  ]);
  const [cursorPos, setCursorPos] = useState(0);
  const { isFocused } = useFocus();
  const focusManager = useFocusManager();

  useInput((input: string) => {
    if (!isFocused) return;

    if (input === 'tab') {
      focusManager.focusNext();
      setCursorPos(0);
    }
    if (input === 'left') {
      setCursorPos((p: number) => Math.max(0, p - 1));
    }
    if (input === 'right') {
      setCursorPos((p: number) => Math.min(p + 1, fields[0].value.length));
    }
    if (input === 'backspace') {
      // Simple backspace for first field
      setFields((f: FormField[]) =>
        f.map((field: FormField, i: number) =>
          i === 0 && field.value.length > 0
            ? { ...field, value: field.value.slice(0, -1) }
            : field
        )
      );
      setCursorPos((p: number) => Math.max(0, p - 1));
    }
    if (input.length === 1 && input.match(/[a-zA-Z0-9@.]/)) {
      setFields((f: FormField[]) =>
        f.map((field: FormField, i: number) =>
          i === 0
            ? { ...field, value: field.value + input }
            : field
        )
      );
      setCursorPos((p: number) => p + 1);
    }
    if (input === 'q' || input === 'Q') {
      process.exit(0);
    }
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Focus Form Demo</Text>
      <Box marginTop={1} flexDirection="column">
        {fields.map((field: FormField, index: number) => (
          <Box key={index} marginY={1}>
            <Text dimColor>{field.label} </Text>
            <Box
              borderStyle="single"
              paddingX={1}
              backgroundColor={index === 0 && isFocused ? 'gray' : undefined}
            >
              <Text>
                {field.value}
                {index === 0 && isFocused && cursorPos >= field.value.length ? '_' : ''}
              </Text>
            </Box>
          </Box>
        ))}
      </Box>
      <Text dimColor marginTop={1}>
        [tab] next | [type] input | [q] quit
      </Text>
    </Box>
  );
}

render(<FocusForm />);
