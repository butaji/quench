/**
 * Counter Island - Hono style example
 *
 * Demonstrates interactive components that hydrate on the client.
 */

import { useState } from "preact/hooks";

interface CounterProps {
  initial?: number;
  step?: number;
}

export default function Counter({ initial = 0, step = 1 }: CounterProps) {
  const [count, setCount] = useState(initial);

  return (
    <div
      style={{
        padding: "1.5rem",
        border: "2px solid #e0e0e0",
        borderRadius: "12px",
        maxWidth: "300px",
        background: "#fafafa",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: "1rem",
          marginBottom: "1rem",
        }}
      >
        <button
          onClick={() => setCount(count - step)}
          style={{
            padding: "0.5rem 1rem",
            fontSize: "1.25rem",
            border: "none",
            borderRadius: "8px",
            background: "#e74c3c",
            color: "white",
            cursor: "pointer",
          }}
        >
          −
        </button>

        <span
          style={{
            fontSize: "2rem",
            fontWeight: "bold",
            minWidth: "3rem",
            textAlign: "center",
          }}
        >
          {count}
        </span>

        <button
          onClick={() => setCount(count + step)}
          style={{
            padding: "0.5rem 1rem",
            fontSize: "1.25rem",
            border: "none",
            borderRadius: "8px",
            background: "#27ae60",
            color: "white",
            cursor: "pointer",
          }}
        >
          +
        </button>
      </div>

      <p style={{ textAlign: "center", color: "#666", fontSize: "0.875rem" }}>
        Step: {step} | Initial: {initial}
      </p>
    </div>
  );
}
