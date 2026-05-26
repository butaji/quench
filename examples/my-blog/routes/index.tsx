/**
 * Home Page
 * 
 * Demonstrates:
 * - Route handler with data
 * - Islands integration
 * - Static components
 */

import { PageProps } from "$fresh/server.ts";
import Counter from "../islands/Counter.tsx";

interface HomeData {
  greeting: string;
  description: string;
  version: string;
  features: string[];
}

export const handler = {
  async GET(_req: Request, _ctx: HandlerContext): Promise<Response> {
    const data: HomeData = {
      greeting: "Welcome to runts!",
      description: "Build lightning-fast web apps with Fresh/Preact and native Rust. Zero external JS runtime, full Fresh compatibility.",
      version: "0.3.0",
      features: [
        "Instant hot reload in development",
        "Native binary compilation for production",
        "Full islands architecture",
        "Fine-grained reactivity with signals",
        "File-based routing like Next.js",
        "Middleware pipeline"
      ]
    };

    return new Response(JSON.stringify(data), {
      headers: {
        "Content-Type": "application/json",
        "X-Runtime": "Rust",
      },
    });
  }
};

export default function Home({ data }: PageProps<HomeData>) {
  return (
    <div class="home-page">
      <header class="hero">
        <h1 style={{ fontSize: "3rem", marginBottom: "1rem" }}>
          {data.greeting}
        </h1>
        <p class="lead" style={{ fontSize: "1.25rem", color: "#666", maxWidth: "600px" }}>
          {data.description}
        </p>
        <p class="version" style={{ marginTop: "0.5rem", color: "#888" }}>
          Version: {data.version}
        </p>
      </header>

      <section class="features" style={{ marginTop: "3rem" }}>
        <h2 style={{ marginBottom: "1rem" }}>Features</h2>
        <ul class="features-list" style={{ listStyle: "none", padding: 0 }}>
          {data.features.map((feature, index) => (
            <li key={index} style={{ 
              padding: "0.75rem", 
              marginBottom: "0.5rem",
              background: index % 2 === 0 ? "#f8f9fa" : "white",
              borderRadius: "8px"
            }}>
              ✓ {feature}
            </li>
          ))}
        </ul>
      </section>

      <section class="demo" style={{ marginTop: "3rem" }}>
        <h2 style={{ marginBottom: "1rem" }}>Interactive Demo</h2>
        <p>Try out the counter island below! This component hydrates on the client.</p>
        
        <div class="demo-item" style={{ marginTop: "1.5rem" }}>
          <h3>Counter Island</h3>
          <Counter initial={5} step={1} label="Try clicking me!" />
        </div>
        
        <div class="demo-item" style={{ marginTop: "1.5rem" }}>
          <h3>Another Counter</h3>
          <Counter initial={100} step={10} label="Step by 10" />
        </div>
      </section>

      <section class="navigation" style={{ marginTop: "3rem", padding: "1.5rem", background: "#f8f9fa", borderRadius: "12px" }}>
        <h3>Explore</h3>
        <div style={{ display: "flex", gap: "1rem", marginTop: "1rem", flexWrap: "wrap" }}>
          <a href="/blog" class="btn btn-primary" style={{
            padding: "0.75rem 1.5rem",
            background: "#1a1a2e",
            color: "white",
            textDecoration: "none",
            borderRadius: "8px",
            fontWeight: "bold"
          }}>
            Read the Blog →
          </a>
          <a href="/about" class="btn btn-secondary" style={{
            padding: "0.75rem 1.5rem",
            background: "white",
            color: "#1a1a2e",
            textDecoration: "none",
            borderRadius: "8px",
            border: "2px solid #1a1a2e",
            fontWeight: "bold"
          }}>
            Learn More
          </a>
        </div>
      </section>
    </div>
  );
}
