// routes/blog/index.tsx - Blog listing page
import { PageProps } from "$fresh/server.ts";

interface BlogData {
  posts: Array<Post>;
  totalCount: number;
}

interface Post {
  slug: string;
  title: string;
  excerpt: string;
}

export default function BlogIndex({ data }: PageProps<BlogData>) {
  const postItems = data.posts.map((post) => {
    return (
      <article key={post.slug} class="post-preview">
        <h2>
          <a href={`/blog/${post.slug}`}>{post.title}</a>
        </h2>
        <p>{post.excerpt}</p>
      </article>
    );
  });
  
  return (
    <div class="blog-page">
      <header class="blog-header">
        <h1>Blog</h1>
        <p>Total posts: {data.totalCount}</p>
      </header>
      
      <main class="blog-list">
        {postItems}
      </main>
    </div>
  );
}
