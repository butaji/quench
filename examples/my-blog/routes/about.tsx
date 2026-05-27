/**
 * About Page
 * 
 * Static page about runts.
 */

import { PageProps } from "$fresh/server.ts";

interface AboutData {
  title: string;
  description: string;
  techStack: string[];
  goals: string[];
  performance: PerformanceMetrics;
}

interface PerformanceMetrics {
  binarySize: string;
  coldStart: string;
  memoryBaseline: string;
}

export const handler = {
  async GET(req: Request, ctx: HandlerContext): Promise<Response> {
    const data: AboutData = {
      title: "About runts",
      description: "runts is a Fresh/Preact-compatible TypeScript framework that compiles to native Rust binaries. Built with a focus on performance, developer experience, and framework compatibility.",
      techStack: [
        "Rust - Core runtime and transpiler",
        "TypeScript - User-facing API",
        "Preact - Component model and hooks",
        "Fresh - Islands architecture and routing",
        "Axum - HTTP server",
        "Tokio - Async runtime"
      ],
      goals: [
        "Zero external JS runtime dependencies",
        "Full Fresh/Preact API compatibility",
        "Sub-100ms hot reload in development",
        "Sub-500KB binary size",
        "Production-ready performance"
      ],
      performance: {
        binarySize: "<2MB",
        coldStart: "<10ms",
        memoryBaseline: "<5MB RSS"
      }
    };

    return ctx.render(data);
  }
};

export default function About({ data }: PageProps<AboutData>) {
  return (
    <div class="about-page">
      <h1 style={{ fontSize: "2.5rem", marginBottom: "1rem" }}>
        {data.title}
      </h1>
      
      <p style={{ fontSize: "1.125rem", lineHeight: 1.7, color: "#333", maxWidth: "700px" }}>
        {data.description}
      </p>

      <section style={{ marginTop: "3rem" }}>
        <h2 style={{ fontSize: "1.5rem", marginBottom: "1rem" }}>
          Technology Stack
        </h2>
        <ul style={{ lineHeight: 2 }}>
          {data.techStack.map((tech, i) => (
            <li key={i}>{tech}</li>
          ))}
        </ul>
      </section>

      <section style={{ marginTop: "3rem" }}>
        <h2 style={{ fontSize: "1.5rem", marginBottom: "1rem" }}>
          Project Goals
        </h2>
        <ul style={{ lineHeight: 2 }}>
          {data.goals.map((goal, i) => (
            <li key={i}>{goal}</li>
          ))}
        </ul>
      </section>

      <section style={{ marginTop: "3rem" }}>
        <h2 style={{ fontSize: "1.5rem", marginBottom: "1rem" }}>
          Performance Targets
        </h2>
        <div style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))",
          gap: "1rem",
          marginTop: "1rem"
        }}>
          <div style={{
            padding: "1.5rem",
            background: "linear-gradient(135deg, #667eea 0%, #764ba2 100%)",
            borderRadius: "12px",
            color: "white",
            textAlign: "center"
          }}>
            <div style={{ fontSize: "2rem", fontWeight: "bold" }}>
              {data.performance.binarySize}
            </div>
            <div style={{ fontSize: "0.875rem", opacity: 0.9 }}>
              Binary Size
            </div>
          </div>
          
          <div style={{
            padding: "1.5rem",
            background: "linear-gradient(135deg, #11998e 0%, #38ef7d 100%)",
            borderRadius: "12px",
            color: "white",
            textAlign: "center"
          }}>
            <div style={{ fontSize: "2rem", fontWeight: "bold" }}>
              {data.performance.coldStart}
            </div>
            <div style={{ fontSize: "0.875rem", opacity: 0.9 }}>
              Cold Start
            </div>
          </div>
          
          <div style={{
            padding: "1.5rem",
            background: "linear-gradient(135deg, #fc4a1a 0%, #f7b733 100%)",
            borderRadius: "12px",
            color: "white",
            textAlign: "center"
          }}>
            <div style={{ fontSize: "2rem", fontWeight: "bold" }}>
              {data.performance.memoryBaseline}
            </div>
            <div style={{ fontSize: "0.875rem", opacity: 0.9 }}>
              Memory Baseline
            </div>
          </div>
        </div>
      </section>

      <section style={{ marginTop: "3rem", padding: "2rem", background: "#f8f9fa", borderRadius: "12px" }}>
        <h2 style={{ fontSize: "1.5rem", marginBottom: "1rem" }}>
          Supported TypeScript Subset
        </h2>
        <p style={{ color: "#666" }}>
          runts supports a well-defined subset of TypeScript that covers 95%+ of real Fresh/Preact usage. 
          This includes JSX/TSX, hooks (useState, useEffect, etc.), async components, signals, 
          file-based routing, and more.
        </p>
        <p style={{ marginTop: "1rem", color: "#666" }}>
          Explicitly excluded features include: class components, eval, dynamic imports, decorators, 
          and complex type inference patterns.
        </p>
      </section>
    </div>
  );
}
