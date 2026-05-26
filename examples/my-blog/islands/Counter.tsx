// islands/Counter.tsx - Interactive counter component
import { useState } from "preact/hooks";
import { signal } from "@preact/signals";

interface CounterProps {
  initial?: number;
  step?: number;
  label?: string;
}

/**
 * Counter - An interactive island demonstrating:
 * - useState hook
 * - Event handlers
 * - Conditional rendering
 * - Props serialization
 */
export default function Counter({ 
  initial = 0, 
  step = 1,
  label = "Counter"
}: CounterProps) {
  const [count, setCount] = useState(initial);
  const [history, setHistory] = useState<number[]>([initial]);
  
  const increment = () => {
    const newValue = count + step;
    setCount(newValue);
    setHistory(prev => [...prev, newValue]);
  };
  
  const decrement = () => {
    const newValue = count - step;
    setCount(newValue);
    setHistory(prev => [...prev, newValue]);
  };
  
  const reset = () => {
    setCount(initial);
    setHistory([initial]);
  };

  const undo = () => {
    if (history.length > 1) {
      const newHistory = [...history];
      newHistory.pop();
      setHistory(newHistory);
      setCount(newHistory[newHistory.length - 1]);
    }
  };

  return (
    <div class="counter">
      <h2>{label}</h2>
      
      <div class="display">
        <span class="count">{count}</span>
        {count !== initial && (
          <span class="delta">({count > initial ? "+" : ""}{count - initial})</span>
        )}
      </div>
      
      <div class="controls">
        <button onClick={decrement}>-</button>
        <button onClick={undo} disabled={history.length <= 1}>Undo</button>
        <button onClick={reset}>Reset</button>
        <button onClick={increment}>+</button>
      </div>
      
      <div class="info">
        <p>Step: {step} | Initial: {initial}</p>
        <p>History length: {history.length}</p>
      </div>
    </div>
  );
}
