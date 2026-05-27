import { PageProps } from "$fresh/server.ts";

interface Post {
  slug: string;
  title: string;
  content: string;
  date: string;
  author: string;
  tags: string[];
}

interface PostData {
  post: Post;
  notFound: boolean;
}

export const handler = {
  async GET(req: Request, ctx: HandlerContext): Promise<Response> {
    const slug = ctx.params.slug;

    if (slug === "introducing-runts") {
      const data: PostData = {
        post: {
          slug: "introducing-runts",
          title: "Introducing runts: Fresh/Preact with Native Rust",
          date: "2026-05-26",
          author: "runts Team",
          tags: ["Rust", "Fresh", "Preact", "Web"],
          content: "Today we are excited to announce runts, a new framework that brings Fresh islands architecture to native Rust compilation."
        },
        notFound: false
      };
      return ctx.render(data);
    }

    if (slug === "islands-architecture") {
      const data: PostData = {
        post: {
          slug: "islands-architecture",
          title: "Understanding the Islands Architecture",
          date: "2026-05-25",
          author: "runts Team",
          tags: ["Architecture", "Performance", "Fresh"],
          content: "The islands architecture is a rendering pattern that enables selective hydration of interactive components while keeping the rest of the page static."
        },
        notFound: false
      };
      return ctx.render(data);
    }

    if (slug === "rust-frontend") {
      const data: PostData = {
        post: {
          slug: "rust-frontend",
          title: "Rust in the Frontend: A New Paradigm",
          date: "2026-05-24",
          author: "runts Team",
          tags: ["Rust", "Compilers", "TypeScript"],
          content: "What if we could compile TypeScript directly to native code? runts explores this question by building a full Fresh/Preact framework on Rust."
        },
        notFound: false
      };
      return ctx.render(data);
    }

    if (slug === "fine-grained-reactivity") {
      const data: PostData = {
        post: {
          slug: "fine-grained-reactivity",
          title: "Fine-Grained Reactivity in Pure Rust",
          date: "2026-05-23",
          author: "runts Team",
          tags: ["Signals", "Reactivity", "Preact"],
          content: "Signals provide a way to express reactive values that automatically track their dependencies and update only what needs to be updated."
        },
        notFound: false
      };
      return ctx.render(data);
    }

    const data: PostData = {
      post: {
        slug: "",
        title: "",
        content: "",
        date: "",
        author: "",
        tags: []
      },
      notFound: true
    };
    return ctx.render(data);
  }
};

export default function BlogPost({ data }: PageProps<PostData>) {
  if (data.notFound) {
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
          {post.tags.map((tag, i) => (
            <span
              key={i}
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
      >
        {post.content}
      </div>

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
