/**
 * Home Page - Hono style
 *
 * Uses Hono's Context API. runts compiles this to native Rust.
 */

import { Context } from "hono";
import Counter from "../islands/Counter.tsx";

interface HomeData {
  greeting: string;
  framework: string;
  runtime: string;
}

export const handler = {
  async GET(_req: Request, c: Context): Promise<Response> {
    const data: HomeData = {
      greeting: "Hello from Hono + runts!",
      framework: "Hono",
      runtime: "Native Rust",
    };
    return c.render(data);
  },
};

export default function Home({ data }: PageProps<HomeData>) {
  return (
    <div class="home-page">
      <header style={{ textAlign: "center", padding: "3rem 1rem" }}>
        <h1 style={{ fontSize: "2.5rem", marginBottom: "1rem" }}>
          {data.greeting}
        </h1>
        <p style={{ fontSize: "1.125rem", color: "#666" }}>
          {data.framework} frontend → {data.runtime} backend
        </p>
      </header>

      <section style={{ maxWidth: "600px", margin: "0 auto", padding: "1rem" }}>
        <h2 style={{ marginBottom: "1rem" }}>What is this?</h2>
        <p style={{ lineHeight: 1.7, color: "#444" }}>
          This example shows a Hono-style TypeScript project that compiles to a
          native Rust binary via runts. Write Hono handlers and JSX components
          in TS/TSX - get a single static binary (&lt;2MB) with zero JS runtime.
        </p>
      </section>

      <section style={{ maxWidth: "600px", margin: "2rem auto", padding: "1rem" }}>
        <h2 style={{ marginBottom: "1rem" }}>Interactive Island</h2>
        <Counter initial={10} step={2} />
      </section>

      <section style={{ maxWidth: "600px", margin: "2rem auto", padding: "1rem" }}>
        <h2 style={{ marginBottom: "1rem" }}>API Route</h2>
        <p style={{ color: "#666" }}>
          Try{" "}
          <code style={{ background: "#f4f4f4", padding: "0.25rem 0.5rem", borderRadius: "4px" }}>
            GET /api/hello
          </code>
        </p>
      </section>
    </div>
  );
}
