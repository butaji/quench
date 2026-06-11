// Form Validation Demo — Quench
// Demonstrates form handling with validation
// Common pattern for CLI configuration tools

import { render, Box, Text, useState, useInput, useApp, useFocus, useFocusManager } from 'ink';

interface Field {
  id: string;
  label: string;
  value: string;
  placeholder: string;
  validator?: (v: string) => string | null;
}

function FormField({ 
  field, 
  isFocused, 
  onChange, 
  onSubmit 
}: { 
  field: Field;
  isFocused: boolean;
  onChange: (value: string) => void;
  onSubmit: () => void;
}): JSX.Element {
  const [cursor, setCursor] = useState(0);

  useInput((input: string, key) => {
    if (!isFocused) return;

    if (key.backspace) {
      if (field.value.length > 0) {
        onChange(field.value.slice(0, -1));
      }
    } else if (key.return) {
      onSubmit();
    } else if (key.tab) {
      // Let focus manager handle
    } else if (input && input.length === 1) {
      // Single character input
      const newValue = field.value + input;
      onChange(newValue);
    }
  });

  const error = field.validator ? field.validator(field.value) : null;
  const borderColor = error ? 'red' : isFocused ? 'cyan' : 'gray';

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text dimColor>{field.label}</Text>
      <Box 
        borderStyle="single" 
        borderColor={borderColor}
        paddingX={1}
      >
        <Text color={field.value.length === 0 ? 'gray' : 'white'}>
          {field.value.length === 0 ? field.placeholder : field.value}
        </Text>
        {isFocused && <Text color="cyan">_</Text>}
      </Box>
      {error && <Text color="red" dimColor>{error}</Text>}
    </Box>
  );
}

function FormValidationDemo(): JSX.Element {
  const [fields, setFields] = useState<Field[]>([
    { 
      id: 'username', 
      label: 'Username', 
      value: '', 
      placeholder: 'Enter username',
      validator: (v) => v.length < 3 ? 'Min 3 characters' : null 
    },
    { 
      id: 'email', 
      label: 'Email', 
      value: '', 
      placeholder: 'Enter email',
      validator: (v) => !v.includes('@') ? 'Invalid email' : null 
    },
    { 
      id: 'age', 
      label: 'Age', 
      value: '', 
      placeholder: 'Enter age',
      validator: (v) => {
        if (v.length === 0) return null;
        const n = parseInt(v);
        return isNaN(n) || n < 1 || n > 150 ? 'Invalid age' : null;
      }
    },
  ]);

  const [focusIdx, setFocusIdx] = useState(0);
  const [submitted, setSubmitted] = useState(false);

  const updateField = (id: string, value: string) => {
    setFields(fs => fs.map(f => f.id === id ? { ...f, value } : f));
  };

  useInput((input: string, key) => {
    if (input === 'q') useApp().exit();
    if (key.tab && !key.shift) {
      setFocusIdx(i => (i + 1) % fields.length);
    }
    if (key.tab && key.shift) {
      setFocusIdx(i => (i - 1 + fields.length) % fields.length);
    }
    if (key.escape) {
      setFocusIdx(-1);
    }
  });

  const handleSubmit = (id: string) => {
    if (focusIdx < fields.length - 1) {
      setFocusIdx(focusIdx + 1);
    } else {
      // Validate all fields
      const hasErrors = fields.some(f => f.validator && f.validator(f.value));
      if (!hasErrors) {
        setSubmitted(true);
      }
    }
  };

  const isValid = fields.every(f => !f.validator || !f.validator(f.value));

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Form Validation Demo</Text>
      <Text dimColor>[Tab] next field | [Esc] unfocus | [q] quit</Text>
      <Text> </Text>

      {submitted ? (
        <Box flexDirection="column" gap={1}>
          <Text bold color="green">Form Submitted!</Text>
          {fields.map(f => (
            <Box key={f.id} flexDirection="row" gap={2}>
              <Text dimColor>{f.label}:</Text>
              <Text>{f.value}</Text>
            </Box>
          ))}
          <Text dimColor>Press [r] to reset</Text>
        </Box>
      ) : (
        <>
          {fields.map((field, idx) => (
            <FormField
              key={field.id}
              field={field}
              isFocused={focusIdx === idx}
              onChange={(v) => updateField(field.id, v)}
              onSubmit={() => handleSubmit(field.id)}
            />
          ))}
          
          <Text> </Text>
          <Text dimColor>Status: {isValid ? <Text color="green">Valid</Text> : <Text color="red">Has errors</Text>}</Text>
        </>
      )}
    </Box>
  );
}

render(<FormValidationDemo />);
