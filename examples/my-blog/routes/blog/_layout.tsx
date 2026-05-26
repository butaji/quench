// routes/blog/_layout.tsx - Blog section layout
import { PageProps } from "$fresh/server.ts";

interface BlogLayoutProps {
  children?: any;
}

/**
 * Blog layout - wraps all blog routes
 * Demonstrates:
 * - Layout nesting
 * - Shared UI
 * - Props from parent
 */
export default function BlogLayout({ children }: BlogLayoutProps) {
  return (
    <div class="blog-layout">
      <aside class="blog-sidebar">
        <nav class="blog-nav">
          <h3>Blog Navigation</h3>
          <ul>
            <li><a href="/blog">All Posts</a></li>
            <li><a href="/blog?filter=recent">Recent</a></li>
            <li><a href="/blog?filter=popular">Popular</a></li>
          </ul>
        </nav>
        
        <div class="blog-categories">
          <h4>Categories</h4>
          <ul>
            <li><a href="/blog?category=tutorial">Tutorials</a></li>
            <li><a href="/blog?category=news">News</a></li>
            <li><a href="/blog?category=guide">Guides</a></li>
          </ul>
        </div>
      </aside>
      
      <main class="blog-content">
        {children}
      </main>
    </div>
  );
}
