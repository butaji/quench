/**
 * Counter Island
 *
 * A fully interactive counter component that demonstrates:
 * - useState hook
 * - Event handlers
 * - Conditional rendering
 * - Proper island architecture
 */

import { useState } from "preact/hooks";

interface CounterProps {
  initial?: number;
  step?: number;
  label?: string;
}

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
    setHistory([...history, newValue]);
  };

  const decrement = () => {
    const newValue = count - step;
    setCount(newValue);
    setHistory([...history, newValue]);
  };

  const reset = () => {
    setCount(initial);
    setHistory([initial]);
  };

  const undo = () => {
    if (history.length > 1) {
      const newHistory = history.slice(0, history.length - 1);
      setCount(newHistory[newHistory.length - 1]);
      setHistory(newHistory);
    }
  };

  const delta = count - initial;
  const canUndo = history.length > 1;
  const canReset = count !== initial;

  return (
    <div class="counter-island" style={{
      padding: "1.5rem",
      border: "2px solid #e0e0e0",
      borderRadius: "12px",
      maxWidth: "400px",
      background: "linear-gradient(135deg, #f5f7fa 0%, #c3cfe2 100%)"
    }}>
      <h2 style={{ margin: "0 0 1rem 0", color: "#333", textAlign: "center" }}>
        {label}
      </h2>

      <div style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: "1rem"
      }}>
        <div class="display" style={{
          display: "flex",
          alignItems: "center",
          gap: "0.5rem",
          fontSize: "2.5rem",
          fontWeight: "bold",
          color: "#1a1a2e"
        }}>
          <span class="count">{count}</span>
          {delta !== 0 && (
            <span style={{
              fontSize: "1rem",
              color: delta > 0 ? "#4caf50" : "#f44336",
              fontWeight: "normal"
            }}>
              ({delta})
            </span>
          )}
        </div>

        <div style={{
          display: "flex",
          gap: "0.5rem",
          flexWrap: "wrap",
          justifyContent: "center"
        }}>
          <button
            onClick={decrement}
            style={{
              padding: "0.5rem 1.25rem",
              fontSize: "1.25rem",
              fontWeight: "bold",
              border: "none",
              borderRadius: "8px",
              background: "#e74c3c",
              color: "white",
              cursor: "pointer"
            }}
          >
            −
          </button>

          <button
            onClick={undo}
            disabled={!canUndo}
            style={{
              padding: "0.5rem 1rem",
              fontSize: "0.875rem",
              border: "none",
              borderRadius: "8px",
              background: canUndo ? "#f39c12" : "#bdc3c7",
              color: "white",
              cursor: canUndo ? "pointer" : "not-allowed",
              opacity: canUndo ? 1 : 0.5
            }}
          >
            Undo
          </button>

          <button
            onClick={reset}
            disabled={!canReset}
            style={{
              padding: "0.5rem 1rem",
              fontSize: "0.875rem",
              border: "none",
              borderRadius: "8px",
              background: canReset ? "#9b59b6" : "#bdc3c7",
              color: "white",
              cursor: canReset ? "pointer" : "not-allowed",
              opacity: canReset ? 1 : 0.5
            }}
          >
            Reset
          </button>

          <button
            onClick={increment}
            style={{
              padding: "0.5rem 1.25rem",
              fontSize: "1.25rem",
              fontWeight: "bold",
              border: "none",
              borderRadius: "8px",
              background: "#27ae60",
              color: "white",
              cursor: "pointer"
            }}
          >
            +
          </button>
        </div>

        <div style={{
          marginTop: "0.5rem",
          fontSize: "0.875rem",
          color: "#666",
          textAlign: "center"
        }}>
          <p style={{ margin: "0.25rem 0" }}>Step: {step} | Initial: {initial}</p>
          <p style={{ margin: "0.25rem 0" }}>History: {history.length} changes</p>
        </div>
      </div>
    </div>
  );
}
