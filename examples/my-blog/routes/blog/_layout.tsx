/**
 * Blog Layout
 * 
 * Wraps all blog routes with consistent styling.
 * The children prop contains the rendered child route.
 */

interface LayoutProps {
  children?: any;
}

export default function BlogLayout({ children }: LayoutProps) {
  return (
    <div class="blog-layout">
      <div class="blog-header" style={{
        padding: "1.5rem 0",
        borderBottom: "2px solid #e0e0e0",
        marginBottom: "2rem"
      }}>
        <h1 style={{ fontSize: "2rem", margin: 0 }}>
          <a href="/blog" style={{ textDecoration: "none", color: "#1a1a2e" }}>
            Blog
          </a>
        </h1>
        <p style={{ color: "#666", margin: "0.5rem 0 0 0" }}>
          Thoughts on Rust, web development, and building fast software
        </p>
      </div>
      
      {children}
    </div>
  );
}
