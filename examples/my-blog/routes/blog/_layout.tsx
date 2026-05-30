/**
 * Blog Layout
 *
 * Wraps all blog routes with children prop.
 */

interface LayoutProps {
  children?: unknown;
}

export default function BlogLayout({ children }: LayoutProps) {
  return (
    <div class="blog-layout">
      <h2>Blog</h2>
      {children}
    </div>
  );
}
