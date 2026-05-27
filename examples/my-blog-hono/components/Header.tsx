/**
 * Header Component - Hono style example
 *
 * Static component rendered server-side.
 */

interface HeaderProps {
  title: string;
}

export default function Header({ title }: HeaderProps) {
  return (
    <header
      style={{
        background: "#1a1a2e",
        color: "white",
        padding: "1rem 2rem",
      }}
    >
      <h1 style={{ fontSize: "1.5rem", margin: 0 }}>{title}</h1>
    </header>
  );
}
