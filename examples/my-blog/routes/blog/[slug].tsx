/**
 * Blog Post Page
 *
 * Dynamic route [slug] for individual posts.
 */

interface Post {
  title: string;
  content: string;
  date: string;
  author: string;
}

interface PostData {
  post: Post | null;
  notFound: boolean;
}

export const handler = {
  async GET(req: Request, ctx: HandlerContext): Promise<Response> {
    const slug = ctx.params.slug;

    const posts: Record<string, Post> = {
      "introducing-runts": {
        title: "Introducing runts",
        content: "runts brings Fresh-style islands architecture to native Rust compilation. No JS runtime needed.",
        date: "2026-05-26",
        author: "runts Team"
      },
      "static-generation": {
        title: "Static Generation in 0.1",
        content: "Version 0.1 generates static HTML from TSX. No client-side interactivity yet.",
        date: "2026-05-25",
        author: "runts Team"
      },
      "file-routing": {
        title: "File-based Routing",
        content: "Routes are determined by file paths. [slug].tsx captures dynamic segments.",
        date: "2026-05-24",
        author: "runts Team"
      }
    };

    const post = posts[slug];
    const data: PostData = {
      post: post || null,
      notFound: !post
    };

    return ctx.render(data);
  }
};

export default function BlogPost({ data }: PageProps<PostData>) {
  if (data.notFound) {
    return (
      <div class="not-found">
        <h1>Post Not Found</h1>
        <a href="/blog">Back to Blog</a>
      </div>
    );
  }

  const post = data.post!;

  return (
    <article class="blog-post">
      <header>
        <h1>{post.title}</h1>
        <div class="meta">
          <time>{post.date}</time>
          <span>by {post.author}</span>
        </div>
      </header>
      <div class="content">
        <p>{post.content}</p>
      </div>
      <footer>
        <a href="/blog">Back to Blog</a>
      </footer>
    </article>
  );
}
