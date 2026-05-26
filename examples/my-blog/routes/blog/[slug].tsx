/**
 * Blog Post Page
 * 
 * Dynamic route for individual blog posts.
 * Uses the [slug] pattern from the filename.
 */

import { PageProps } from "$fresh/server.ts";

// Static post data for demonstration
const posts: Record<string, {
  title: string;
  content: string;
  date: string;
  author: string;
  tags: string[];
}> = {
  "introducing-runts": {
    title: "Introducing runts: Fresh/Preact with Native Rust",
    date: "2026-05-26",
    author: "runts Team",
    tags: ["Rust", "Fresh", "Preact", "Web"],
    content: `
      <p>Today we're excited to announce <strong>runts</strong>, a new framework that brings Fresh's islands architecture to native Rust compilation.</p>
      
      <h2>What is runts?</h2>
      <p>runts is a Fresh/Preact-compatible TypeScript framework that compiles to native Rust binaries. It provides zero external JS runtime (no V8, Deno, or WebAssembly JS) while maintaining full Fresh/Preact API compatibility.</p>
      
      <h2>Key Features</h2>
      <ul>
        <li><strong>Native Binary Output</strong> - Compile to efficient Rust binaries</li>
        <li><strong>Zero External Runtime</strong> - No V8, Deno, or Wasm JS engines</li>
        <li><strong>Full Fresh Compatibility</strong> - Islands, layouts, middleware, routes</li>
        <li><strong>Instant Hot Reload</strong> - HIR interpretation in development</li>
        <li><strong>Fine-Grained Reactivity</strong> - Preact Signals compatible</li>
      </ul>
      
      <h2>How It Works</h2>
      <p>In development mode, runts parses TypeScript/TSX to a High-Level IR (HIR) and interprets it directly. This enables instant hot reload without any Rust recompilation.</p>
      
      <p>In production, runts transpiles TypeScript to Rust source code via in-memory code generation, then compiles with cargo for maximum performance.</p>
      
      <h2>Getting Started</h2>
      <p>Check out the examples in the repository to see runts in action. The my-blog example demonstrates routing, islands, layouts, and full SSR.</p>
    `
  },
  "islands-architecture": {
    title: "Understanding the Islands Architecture",
    date: "2026-05-25",
    author: "runts Team",
    tags: ["Architecture", "Performance", "Fresh"],
    content: `
      <p>The islands architecture is a rendering pattern that enables selective hydration of interactive components while keeping the rest of the page static.</p>
      
      <h2>The Problem with Traditional SSR</h2>
      <p>Server-side rendering improves initial page load and SEO, but traditional SSR often ships all JavaScript to the client, even for static content. This leads to large JavaScript bundles and slow Time-to-Interactive.</p>
      
      <h2>The Islands Solution</h2>
      <p>Islands architecture solves this by marking only interactive components for client hydration. The rest of the page remains as static HTML, rendered once on the server.</p>
      
      <pre><code>// Static HTML (no hydration)
<div>Just static content</div>

// Island (will hydrate on client)
<div data-island="Counter" data-id="abc123">
  <button>0</button>
</div></code></pre>
      
      <h2>Hydration Strategies</h2>
      <p>runts supports multiple hydration strategies:</p>
      <ul>
        <li><strong>Eager</strong> - Hydrate immediately on page load</li>
        <li><strong>Visible</strong> - Hydrate when the island enters the viewport</li>
        <li><strong>Idle</strong> - Hydrate during browser idle time</li>
        <li><strong>Manual</strong> - Hydrate on explicit trigger</li>
      </ul>
    `
  },
  "rust-frontend": {
    title: "Rust in the Frontend: A New Paradigm",
    date: "2026-05-24",
    author: "runts Team",
    tags: ["Rust", "Compilers", "TypeScript"],
    content: `
      <p>What if we could compile TypeScript directly to native code? runts explores this question by building a full Fresh/Preact framework on Rust.</p>
      
      <h2>The Transpilation Pipeline</h2>
      <p>runts implements a multi-stage transpilation pipeline:</p>
      
      <ol>
        <li><strong>Parse</strong> - TSX → AST using custom recursive descent parser</li>
        <li><strong>Analyze</strong> - Semantic analysis, type checking, hook detection</li>
        <li><strong>Transform</strong> - Convert to High-Level IR (HIR)</li>
        <li><strong>Generate</strong> - HIR → Rust source code</li>
        <li><strong>Compile</strong> - cargo build for native binary</li>
      </ol>
      
      <h2>Why Rust?</h2>
      <p>Rust offers several advantages for frontend tooling:</p>
      <ul>
        <li><strong>Performance</strong> - Native code without garbage collection pauses</li>
        <li><strong>Memory Safety</strong> - No buffer overflows, use-after-free, etc.</li>
        <li><strong>Binary Distribution</strong> - Single executable, no runtime dependencies</li>
        <li><strong>Tooling</strong> - Excellent compiler, rustfmt, clippy</li>
      </ul>
    `
  },
  "fine-grained-reactivity": {
    title: "Fine-Grained Reactivity in Pure Rust",
    date: "2026-05-23",
    author: "runts Team",
    tags: ["Signals", "Reactivity", "Preact"],
    content: `
      <p>Signals provide a way to express reactive values that automatically track their dependencies and update only what needs to be updated.</p>
      
      <h2>How Signals Work</h2>
      <p>A signal is a container for a reactive value. When you read a signal's value, you implicitly subscribe to updates. When you write to a signal, all subscribers are notified.</p>
      
      <pre><code>// Create a signal
let count = signal(0);

// Read the value
let n = count.value; // 0

// Update triggers all subscribers
count.value = 5;</code></pre>
      
      <h2>Computed Values</h2>
      <p>Computed values derive from other signals and automatically update when their dependencies change:</p>
      
      <pre><code>let doubled = computed(|| count.value * 2);</code></pre>
      
      <h2>Effects</h2>
      <p>Effects run side effects when their signal dependencies change:</p>
      
      <pre><code>effect(|| {
  console.log("Count changed:", count.value);
});</code></pre>
    `
  }
};

interface PostData {
  post: {
    slug: string;
    title: string;
    content: string;
    date: string;
    author: string;
    tags: string[];
  } | null;
}

export const handler = {
  async GET(_req: Request, _ctx: HandlerContext): Promise<Response> {
    const slug = _ctx.params.slug;
    
    const post = posts[slug] || null;
    
    if (!post) {
      return new Response(JSON.stringify({ post: null }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    }

    const data: PostData = {
      post: { slug, ...post }
    };

    return new Response(JSON.stringify(data), {
      headers: { "Content-Type": "application/json" },
    });
  }
};

export default function BlogPost({ data, params }: PageProps<PostData>) {
  if (!data.post) {
    return (
      <div class="not-found">
        <h1>Post Not Found</h1>
        <p>The requested blog post could not be found.</p>
        <a href="/blog">← Back to Blog</a>
      </div>
    );
  }

  const { post } = data;

  return (
    <article class="blog-post">
      <header style={{ marginBottom: "2rem" }}>
        <div style={{ 
          display: "flex", 
          alignItems: "center", 
          gap: "1rem",
          marginBottom: "1rem",
          fontSize: "0.875rem",
          color: "#666"
        }}>
          <time>{post.date}</time>
          <span>•</span>
          <span>By {post.author}</span>
        </div>
        
        <h1 style={{ 
          fontSize: "2.5rem", 
          margin: 0,
          lineHeight: 1.2
        }}>
          {post.title}
        </h1>
        
        <div style={{ 
          display: "flex", 
          gap: "0.5rem",
          marginTop: "1rem",
          flexWrap: "wrap"
        }}>
          {post.tags.map((tag) => (
            <span 
              key={tag}
              style={{
                padding: "0.25rem 0.75rem",
                background: "#e8f4f8",
                color: "#2980b9",
                borderRadius: "20px",
                fontSize: "0.875rem"
              }}
            >
              {tag}
            </span>
          ))}
        </div>
      </header>
      
      <div 
        class="post-content"
        style={{ lineHeight: 1.8 }}
        dangerouslySetInnerHTML={{ __html: post.content }}
      />
      
      <footer style={{ 
        marginTop: "3rem",
        paddingTop: "2rem",
        borderTop: "1px solid #e0e0e0"
      }}>
        <a 
          href="/blog" 
          style={{
            color: "#3498db",
            textDecoration: "none",
            fontWeight: "500"
          }}
        >
          ← Back to Blog
        </a>
      </footer>
    </article>
  );
}
