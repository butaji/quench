// routes/blog/[slug].tsx - Individual blog post
import { PageProps, HandlerContext } from "$fresh/server.ts";
import Counter from "../../islands/Counter.tsx";

interface Post {
  slug: string;
  title: string;
  content: string;
  date: string;
  author: Author;
  category: string;
  tags: string[];
  readTime: number;
  likes: number;
}

interface Author {
  name: string;
  avatar: string;
  bio: string;
}

interface PostData {
  post: Post | null;
  relatedPosts: Post[];
}

// Simulated database
const POSTS: Record<string, Post> = {
  "getting-started-with-rusts": {
    slug: "getting-started-with-rusts",
    title: "Getting Started with Rust's runts",
    content: `
# Introduction

runts is a revolutionary framework that compiles Fresh/Preact TypeScript to native Rust binaries. 
This means you get the best of both worlds:

- **Fast development** with TypeScript and JSX
- **Native performance** with zero-runtime overhead
- **Small binaries** that deploy anywhere

## Getting Started

First, install runts:

\`\`\`bash
cargo install runts
\`\`\`

Then create a new project:

\`\`\`bash
runts init my-app
cd my-app
runts dev
\`\`\`

## Your First Island

Islands are interactive components that ship JavaScript:

\`\`\`tsx
// islands/Counter.tsx
import { useState } from "preact/hooks";

export default function Counter() {
  const [count, setCount] = useState(0);
  
  return (
    <div>
      <p>Count: {count}</p>
      <button onClick={() => setCount(c => c + 1)}>+</button>
    </div>
  );
}
\`\`\`

## Static vs Interactive

Notice how only the Counter island loads JavaScript. The rest of your page is pure HTML!

Try clicking the counter below:
    `,
    date: "2024-01-15",
    author: {
      name: "Jane Developer",
      avatar: "/avatars/jane.png",
      bio: "Rust enthusiast and web developer",
    },
    category: "tutorial",
    tags: ["rust", "typescript", "fresh", "runts"],
    readTime: 8,
    likes: 42,
  },
  "islands-architecture-deep-dive": {
    slug: "islands-architecture-deep-dive",
    title: "Islands Architecture Deep Dive",
    content: `
# Understanding Islands Architecture

The islands architecture is a pattern for building performant web applications. 
It's especially powerful when combined with server-side rendering.

## The Problem

Traditional React apps send all JavaScript to the client, even for static content. 
This leads to:

- Slow initial page loads
- High JavaScript bundle sizes
- Poor performance on mobile devices

## The Solution: Islands

Islands architecture solves this by:

1. **Server-rendering everything** - All content is rendered on the server
2. **Hydrating only interactive parts** - Only "islands" of interactivity load JavaScript
3. **Progressive enhancement** - Pages are usable even before JavaScript loads

## Try It

Interact with this counter to see islands in action:
    `,
    date: "2024-01-10",
    author: {
      name: "John Engineer",
      avatar: "/avatars/john.png",
      bio: "Performance optimization specialist",
    },
    category: "guide",
    tags: ["architecture", "performance", "hydration"],
    readTime: 12,
    likes: 89,
  },
  "building-a-blog-with-fresh": {
    slug: "building-a-blog-with-fresh",
    title: "Building a Blog with Fresh and runts",
    content: `
# Building a Blog with Fresh and runts

In this tutorial, we'll build a complete blog application using Fresh patterns 
and compile it to a native Rust binary with runts.

## Project Structure

\`\`\`
my-blog/
├── routes/
│   ├── index.tsx          # Home page
│   ├── about.tsx         # About page
│   └── blog/
│       ├── index.tsx     # Blog listing
│       └── [slug].tsx    # Blog posts
├── islands/
│   ├── Counter.tsx       # Interactive counter
│   └── TodoList.tsx      # Todo app
└── components/
    └── Header.tsx        # Static header
\`\`\`

## File-Based Routing

Fresh uses file-based routing. Each file in \`routes/\` becomes a route.

## Islands

Islands in \`islands/\` are interactive components. Everything else is static.

Check out this counter to see it in action:
    `,
    date: "2024-01-05",
    author: {
      name: "Sarah Coder",
      avatar: "/avatars/sarah.png",
      bio: "Full-stack developer and educator",
    },
    category: "tutorial",
    tags: ["fresh", "tutorial", "blog"],
    readTime: 15,
    likes: 67,
  },
};

/**
 * Blog post handler
 */
export const handler = {
  async GET(req: Request, ctx: HandlerContext): Promise<Response> {
    const { slug } = ctx.params;
    
    const post = POSTS.get(slug);
    
    if (!post) {
      // Return 404
      return new Response(JSON.stringify({ post: null, relatedPosts: [] }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    }
    
    // Get related posts (excluding current)
    const relatedPosts = Object.values(POSTS)
      .filter(p => p.slug !== slug && p.category === post.category)
      .slice(0, 3);
    
    const data: PostData = {
      post,
      relatedPosts,
    };
    
    return new Response(JSON.stringify(data), {
      headers: { "Content-Type": "application/json" },
    });
  },
};

export default function BlogPost({ data }: PageProps<PostData>) {
  if (!data.post) {
    return (
      <div class="post-not-found">
        <h1>Post Not Found</h1>
        <p>Sorry, the requested blog post could not be found.</p>
        <a href="/blog">← Back to Blog</a>
      </div>
    );
  }
  
  const { post, relatedPosts } = data;
  
  // Parse markdown content (simplified - in production use a proper parser)
  const paragraphs = post.content.trim().split("\n\n");
  
  return (
    <article class="blog-post">
      <header class="post-header">
        <div class="post-meta">
          <span class="post-category">{post.category}</span>
          <time class="post-date">{post.date}</time>
          <span class="post-read-time">{post.readTime} min read</span>
        </div>
        <h1>{post.title}</h1>
        <div class="author">
          <img src={post.author.avatar} alt={post.author.name} class="author-avatar" />
          <div class="author-info">
            <span class="author-name">{post.author.name}</span>
            <span class="author-bio">{post.author.bio}</span>
          </div>
        </div>
      </header>
      
      <div class="post-content">
        {paragraphs.map((p, i) => {
          if (p.startsWith("# ")) {
            return <h2 key={i}>{p.slice(2)}</h2>;
          }
          if (p.startsWith("## ")) {
            return <h3 key={i}>{p.slice(3)}</h3>;
          }
          if (p.startsWith("Try clicking")) {
            return (
              <div key={i}>
                <p>{p}</p>
                <div class="interactive-demo">
                  <Counter initial={post.likes} step={1} label="Like Counter 🎉" />
                </div>
              </div>
            );
          }
          return <p key={i}>{p}</p>;
        })}
      </div>
      
      <footer class="post-footer">
        <div class="tags">
          {post.tags.map(tag => (
            <span key={tag} class="tag">#{tag}</span>
          ))}
        </div>
        
        <div class="likes">
          <span>{post.likes} likes</span>
        </div>
      </footer>
      
      {relatedPosts.length > 0 && (
        <aside class="related-posts">
          <h3>Related Posts</h3>
          <ul>
            {relatedPosts.map(related => (
              <li key={related.slug}>
                <a href={`/blog/${related.slug}`}>{related.title}</a>
              </li>
            ))}
          </ul>
        </aside>
      )}
    </article>
  );
}
