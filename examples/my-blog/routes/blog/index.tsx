/**
 * Blog Index Page
 *
 * Lists all blog posts.
 */

import { PageProps, HandlerContext } from "$fresh/server.ts";

interface Post {
  slug: string;
  title: string;
  excerpt: string;
  date: string;
}

interface BlogData {
  posts: Post[];
}

export const handler = {
  async GET(req: Request, ctx: HandlerContext): Promise<Response> {
    const posts: Post[] = [
      {
        slug: "introducing-runts",
        title: "Introducing runts",
        excerpt: "Build fast web apps with Fresh and native Rust.",
        date: "2026-05-26"
      },
      {
        slug: "static-generation",
        title: "Static Generation in 0.1",
        excerpt: "How runts compiles TSX to static HTML.",
        date: "2026-05-25"
      },
      {
        slug: "file-routing",
        title: "File-based Routing",
        excerpt: "Routes map directly to file paths.",
        date: "2026-05-24"
      }
    ];

    const data: BlogData = { posts };

    return ctx.render(data);
  }
};

export default function BlogIndex({ data }: PageProps<BlogData>) {
  return (
    <div class="blog-index">
      <h2>Blog Posts</h2>
      <ul class="posts-list">
        {data.posts.map((post) => (
          <li key={post.slug} class="post-item">
            <a href={`/blog/${post.slug}`}>
              <h3>{post.title}</h3>
            </a>
            <p>{post.excerpt}</p>
            <time>{post.date}</time>
          </li>
        ))}
      </ul>
    </div>
  );
}
