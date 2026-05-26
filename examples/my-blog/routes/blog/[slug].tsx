// routes/blog/[slug].tsx - Individual blog post
import { PageProps } from "$fresh/server.ts";

interface Post {
  title: string;
  content: string;
  author: string;
  date: string;
}

export default function BlogPost({ params, data }: PageProps<Post>) {
  return (
    <article class="blog-post">
      <header>
        <h1>{data.title}</h1>
        <p class="meta">
          By {data.author} on {data.date}
        </p>
      </header>
      
      <div class="content">
        {data.content}
      </div>
      
      <footer>
        <a href="/blog">Back to blog</a>
      </footer>
    </article>
  );
}
