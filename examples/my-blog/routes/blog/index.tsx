// routes/blog/index.tsx - Blog listing page
import { PageProps, HandlerContext } from "$fresh/server.ts";

interface Post {
  slug: string;
  title: string;
  excerpt: string;
  date: string;
  category: string;
  readTime: number;
}

interface BlogData {
  posts: Post[];
  total: number;
  page: number;
}

/**
 * Blog index handler - GET request
 */
export const handler = {
  async GET(req: Request, ctx: HandlerContext): Promise<Response> {
    const url = new URL(req.url);
    const page = parseInt(url.searchParams.get("page") || "1");
    const filter = url.searchParams.get("filter") || "recent";
    const category = url.searchParams.get("category");

    // Simulate database query
    const allPosts: Post[] = [
      {
        slug: "getting-started-with-rusts",
        title: "Getting Started with Rust's runts",
        excerpt: "Learn how to build lightning-fast web applications using the runts framework with Fresh/Preact compatibility.",
        date: "2024-01-15",
        category: "tutorial",
        readTime: 8,
      },
      {
        slug: "islands-architecture-deep-dive",
        title: "Islands Architecture Deep Dive",
        excerpt: "Understanding partial hydration and how islands architecture enables optimal performance.",
        date: "2024-01-10",
        category: "guide",
        readTime: 12,
      },
      {
        slug: "building-a-blog-with-fresh",
        title: "Building a Blog with Fresh and runts",
        excerpt: "A complete tutorial on building a blog application using Fresh framework patterns compiled to Rust.",
        date: "2024-01-05",
        category: "tutorial",
        readTime: 15,
      },
      {
        slug: "why-rust-for-web-dev",
        title: "Why Rust for Web Development?",
        excerpt: "Exploring the benefits of compiling to native binaries for web applications.",
        date: "2024-01-01",
        category: "opinion",
        readTime: 6,
      },
    ];

    // Apply filters
    let filteredPosts = allPosts;
    if (category) {
      filteredPosts = filteredPosts.filter(p => p.category === category);
    }
    if (filter === "popular") {
      // Sort by read time as a proxy for popularity
      filteredPosts = [...filteredPosts].sort((a, b) => b.readTime - a.readTime);
    }

    const data: BlogData = {
      posts: filteredPosts,
      total: filteredPosts.length,
      page,
    };

    return new Response(JSON.stringify(data), {
      headers: { "Content-Type": "application/json" },
    });
  },
};

export default function BlogIndex({ data }: PageProps<BlogData>) {
  return (
    <div class="blog-index">
      <header class="blog-header">
        <h1>Blog</h1>
        <p>Articles about runts, Rust, and web development</p>
      </header>

      <div class="posts-list">
        {data.posts.length === 0 ? (
          <p class="no-posts">No posts found.</p>
        ) : (
          data.posts.map((post) => (
            <article key={post.slug} class="post-card">
              <header class="post-header">
                <span class="post-category">{post.category}</span>
                <time class="post-date">{post.date}</time>
              </header>
              <h2>
                <a href={`/blog/${post.slug}`}>{post.title}</a>
              </h2>
              <p class="post-excerpt">{post.excerpt}</p>
              <footer class="post-footer">
                <span class="read-time">{post.readTime} min read</span>
                <a href={`/blog/${post.slug}`} class="read-more">
                  Read more →
                </a>
              </footer>
            </article>
          ))
        )}
      </div>

      <nav class="pagination">
        {data.page > 1 && (
          <a href={`/blog?page=${data.page - 1}`} class="page-link">
            ← Previous
          </a>
        )}
        <span class="page-info">
          Page {data.page} of {Math.ceil(data.total / 5)}
        </span>
        {data.page < Math.ceil(data.total / 5) && (
          <a href={`/blog?page=${data.page + 1}`} class="page-link">
            Next →
          </a>
        )}
      </nav>
    </div>
  );
}
