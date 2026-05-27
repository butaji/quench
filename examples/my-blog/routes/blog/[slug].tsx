import { PageProps } from "$fresh/server.ts";

const posts: Record<string, any> = {
  "introducing-runts": {
    title: "Introducing runts: Fresh/Preact with Native Rust",
    date: "2026-05-26",
    author: "runts Team",
    tags: ["Rust", "Fresh", "Preact", "Web"],
    content: "<p>Today we are excited to announce <strong>runts</strong>, a new framework that brings Freshs islands architecture to native Rust compilation.</p><h2>What is runts?</h2><p>runts is a Fresh/Preact-compatible TypeScript framework that compiles to native Rust binaries.</p><h2>Key Features</h2><ul><li>Native Binary Output</li><li>Zero External Runtime</li><li>Full Fresh Compatibility</li><li>Instant Hot Reload</li><li>Fine-Grained Reactivity</li></ul>"
  },
  "islands-architecture": {
    title: "Understanding the Islands Architecture",
    date: "2026-05-25",
    author: "runts Team",
    tags: ["Architecture", "Performance", "Fresh"],
    content: "<p>The islands architecture is a rendering pattern that enables selective hydration of interactive components while keeping the rest of the page static.</p><h2>The Problem with Traditional SSR</h2><p>Server-side rendering improves initial page load and SEO, but traditional SSR often ships all JavaScript to the client, even for static content.</p><h2>The Islands Solution</h2><p>Islands architecture solves this by marking only interactive components for client hydration.</p><h2>Hydration Strategies</h2><ul><li>Eager - Hydrate immediately on page load</li><li>Visible - Hydrate when the island enters the viewport</li><li>Idle - Hydrate during browser idle time</li><li>Manual - Hydrate on explicit trigger</li></ul>"
  },
  "rust-frontend": {
    title: "Rust in the Frontend: A New Paradigm",
    date: "2026-05-24",
    author: "runts Team",
    tags: ["Rust", "Compilers", "TypeScript"],
    content: "<p>What if we could compile TypeScript directly to native code? runts explores this question by building a full Fresh/Preact framework on Rust.</p><h2>The Transpilation Pipeline</h2><ol><li>Parse - TSX to AST using custom recursive descent parser</li><li>Analyze - Semantic analysis, type checking, hook detection</li><li>Transform - Convert to High-Level IR (HIR)</li><li>Generate - HIR to Rust source code</li><li>Compile - cargo build for native binary</li></ol><h2>Why Rust?</h2><ul><li>Performance - Native code without garbage collection pauses</li><li>Memory Safety - No buffer overflows, use-after-free, etc.</li><li>Binary Distribution - Single executable, no runtime dependencies</li><li>Tooling - Excellent compiler, rustfmt, clippy</li></ul>"
  },
  "fine-grained-reactivity": {
    title: "Fine-Grained Reactivity in Pure Rust",
    date: "2026-05-23",
    author: "runts Team",
    tags: ["Signals", "Reactivity", "Preact"],
    content: "<p>Signals provide a way to express reactive values that automatically track their dependencies and update only what needs to be updated.</p><h2>How Signals Work</h2><p>A signal is a container for a reactive value. When you read a signals value, you implicitly subscribe to updates. When you write to a signal, all subscribers are notified.</p><h2>Computed Values</h2><p>Computed values derive from other signals and automatically update when their dependencies change.</p><h2>Effects</h2><p>Effects run side effects when their signal dependencies change.</p>"
  }
};

interface PostData {
  post: any;
}

export const handler = {
  async GET(_req: Request, _ctx: HandlerContext): Promise<Response> {
    const slug = _ctx.params.slug;
    const post = posts[slug];

    if (!post) {
      return new Response(JSON.stringify({ post: null }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    }

    return new Response(JSON.stringify({
      post: {
        slug: slug,
        title: post.title,
        content: post.content,
        date: post.date,
        author: post.author,
        tags: post.tags
      }
    }), {
      headers: { "Content-Type": "application/json" },
    });
  }
};

export default function BlogPost({ data }: PageProps<PostData>) {
  if (!data.post) {
    return (
      <div class="not-found">
        <h1>Post Not Found</h1>
        <p>The requested blog post could not be found.</p>
        <a href="/blog">Back to Blog</a>
      </div>
    );
  }

  const post = data.post;

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
          Back to Blog
        </a>
      </footer>
    </article>
  );
}
