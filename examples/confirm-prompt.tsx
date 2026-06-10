// Confirm Prompt — TuiBridge
// Yes/No confirmation dialog pattern
// Common in CLI tools for destructive actions

import { render, Box, Text, useState, useInput, useApp } from 'ink';

interface ConfirmOptions {
  message: string;
  defaultValue?: boolean;
}

function ConfirmPrompt({ message, defaultValue = true }: ConfirmOptions): JSX.Element {
  const [selected, setSelected] = useState(defaultValue);

  useInput((input: string, key: { leftArrow: boolean; rightArrow: boolean; return: boolean }) => {
    if (key.leftArrow || key.rightArrow || input === 'h' || input === 'l') {
      setSelected(s => !s);
    }
    if (key.return || input === ' ') {
      // Confirmed - exit handled by parent
    }
  });

  return (
    <Box flexDirection="row" gap={2}>
      <Text color={selected ? 'cyan' : 'gray'}>
        {selected ? '► Yes' : '  Yes'}
      </Text>
      <Text color={!selected ? 'cyan' : 'gray'}>
        {!selected ? '► No' : '  No'}
      </Text>
    </Box>
  );
}

function ConfirmDemo(): JSX.Element {
  const [step, setStep] = useState<'initial' | 'confirm' | 'result'>('initial');
  const [confirmed, setConfirmed] = useState<boolean | null>(null);

  const handleConfirm = (value: boolean) => {
    setConfirmed(value);
    setStep('result');
  };

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (step === 'initial' && input === 'y') {
      setStep('confirm');
    }
    if (step === 'result' && input === ' ') {
      setStep('initial');
      setConfirmed(null);
    }
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Confirm Prompt Demo</Text>
      <Text dimColor>[q] quit</Text>
      <Text> </Text>

      {step === 'initial' && (
        <Box flexDirection="column" gap={1}>
          <Text>Ready to delete the database?</Text>
          <Text dimColor>Press <Text bold>[y]</Text> to confirm</Text>
        </Box>
      )}

      {step === 'confirm' && (
        <Box flexDirection="column" gap={1}>
          <Box borderStyle="single" borderColor="red" padding={1}>
            <Text bold color="red">⚠ Warning!</Text>
            <Text> This will permanently delete all data.</Text>
          </Box>
          <Text>Are you sure?</Text>
          <Text> </Text>
          <ConfirmPrompt 
            message="Delete database?" 
            defaultValue={false} 
          />
          <Text dimColor>[←/→ or h/l] select | [Enter] confirm</Text>
        </Box>
      )}

      {step === 'result' && (
        <Box flexDirection="column" gap={1}>
          {confirmed ? (
            <Box borderStyle="single" borderColor="green" padding={1}>
              <Text color="green">✓ Database deleted successfully</Text>
            </Box>
          ) : (
            <Box borderStyle="single" borderColor="yellow" padding={1}>
              <Text color="yellow">✗ Operation cancelled</Text>
            </Box>
          )}
          <Text dimColor>Press [Space] to continue</Text>
        </Box>
      )}
    </Box>
  );
}

render(<ConfirmDemo />);
