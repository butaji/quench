/**
 * Counter Island
 *
 * Non-interactive for 0.1. Just displays props.
 */

interface CounterProps {
  initial?: number;
  step?: number;
}

export default function Counter({ initial = 0, step = 1 }: CounterProps) {
  return (
    <div class="counter">
      <p>Count: {initial} (interactive in v0.2)</p>
      <p>Step: {step}</p>
    </div>
  );
}
