// Wizard Example — TuiBridge
// Demonstrates useMemo, useCallback, multi-step flow

import { render, Box, Text, useState, useMemo, useCallback, useInput, useApp } from 'ink';

interface WizardData {
  name: string;
  email: string;
  role: string;
}

function Wizard(): JSX.Element {
  const [step, setStep] = useState(0);
  const [data, setData] = useState<WizardData>({ name: '', email: '', role: 'dev' });

  const steps = ['Name', 'Email', 'Role', 'Review'];

  const canNext = useMemo(() => {
    if (step === 0) return data.name.length > 0;
    if (step === 1) return data.email.length > 0;
    if (step === 2) return true;
    return false;
  }, [step, data]);

  const canPrev = useMemo(() => step > 0, [step]);

  const next = useCallback(() => {
    if (canNext) setStep(s => Math.min(s + 1, steps.length - 1));
  }, [canNext]);

  const prev = useCallback(() => {
    if (canPrev) setStep(s => Math.max(s - 1, 0));
  }, [canPrev]);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'l' || input === 'rightArrow') next();
    if (input === 'h' || input === 'leftArrow') prev();
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Setup Wizard</Text>
      <Box flexDirection="row" marginY={1}>
        {steps.map((s, i) => {
          const color = i === step ? 'yellow' : (i < step ? 'green' : 'gray');
          const marker = i === step ? '► ' : (i < step ? '✓ ' : '○ ');
          return <Text key={i} color={color}>{marker}{s}  </Text>;
        })}
      </Box>
      {step === 0 && (
        <Box flexDirection="column">
          <Text>Enter your name:</Text>
          <Text bold>{data.name || '(empty)'}</Text>
        </Box>
      )}
      {step === 1 && (
        <Box flexDirection="column">
          <Text>Enter your email:</Text>
          <Text bold>{data.email || '(empty)'}</Text>
        </Box>
      )}
      {step === 2 && (
        <Box flexDirection="column">
          <Text>Select your role:</Text>
          <Text bold>{data.role}</Text>
        </Box>
      )}
      {step === 3 && (
        <Box flexDirection="column" borderStyle="single" padding={1}>
          <Text bold>Review</Text>
          <Text>Name:  {data.name}</Text>
          <Text>Email: {data.email}</Text>
          <Text>Role:  {data.role}</Text>
        </Box>
      )}
      <Text> </Text>
      <Text dimColor>[h/←] back | [l/→] next | [q] quit</Text>
    </Box>
  );
}

render(<Wizard />);
