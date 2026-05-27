/**
 * Blog Index Page
 *
 * Lists all blog posts with pre-rendered content.
 */

import { PageProps } from "$fresh/server.ts";

interface Post {
  slug: string;
  title: string;
  excerpt: string;
  date: string;
  readingTime: string;
}

interface BlogData {
  posts: Post[];
}

export const handler = {
  async GET(req: Request, ctx: HandlerContext): Promise<Response> {
    const posts: Post[] = [
      {
        slug: "introducing-runts",
        title: "Introducing runts: Fresh/Preact with Native Rust",
        excerpt: "Build lightning-fast web applications with Fresh's islands architecture, compiled to native Rust binaries.",
        date: "2026-05-26",
        readingTime: "5 min read"
      },
      {
        slug: "islands-architecture",
        title: "Understanding the Islands Architecture",
        excerpt: "How selective hydration enables partial page interactivity while maintaining excellent performance.",
        date: "2026-05-25",
        readingTime: "8 min read"
      },
      {
        slug: "rust-frontend",
        title: "Rust in the Frontend: A New Paradigm",
        excerpt: "Exploring the benefits and challenges of compiling TypeScript to Rust.",
        date: "2026-05-24",
        readingTime: "12 min read"
      },
      {
        slug: "fine-grained-reactivity",
        title: "Fine-Grained Reactivity in Pure Rust",
        excerpt: "Implementing Preact Signals and computed values in Rust for efficient reactive updates.",
        date: "2026-05-23",
        readingTime: "10 min read"
      }
    ];

    const data: BlogData = { posts: posts };

    return ctx.render(data);
  }
};

export default function BlogIndex({ data }: PageProps<BlogData>) {
  return (
    <div class="blog-index">
      <div class="posts-list">
        {data.posts.map((post) => (
          <article key={post.slug} class="post-card" style={{
            marginBottom: "2rem",
            padding: "1.5rem",
            border: "1px solid #e0e0e0",
            borderRadius: "12px",
            transition: "box-shadow 0.2s",
          }}>
            <header style={{ marginBottom: "1rem" }}>
              <div style={{
                display: "flex",
                alignItems: "center",
                gap: "1rem",
                marginBottom: "0.5rem",
                fontSize: "0.875rem",
                color: "#666"
              }}>
                <time>{post.date}</time>
                <span>•</span>
                <span>{post.readingTime}</span>
              </div>
              <h2 style={{ margin: 0 }}>
                <a
                  href="/blog"
                  style={{
                    textDecoration: "none",
                    color: "#1a1a2e",
                    fontSize: "1.5rem"
                  }}
                >
                  {post.title}
                </a>
              </h2>
            </header>
            <p style={{
              color: "#555",
              lineHeight: 1.7,
              margin: 0
            }}>
              {post.excerpt}
            </p>
            <footer style={{ marginTop: "1rem" }}>
              <a
                href="/blog"
                style={{
                  color: "#3498db",
                  textDecoration: "none",
                  fontWeight: "500"
                }}
              >
                Read more →
              </a>
            </footer>
          </article>
        ))}
      </div>
    </div>
  );
}
