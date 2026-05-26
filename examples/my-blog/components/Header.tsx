// components/Header.tsx - Static header component
import { IS_BROWSER } from "$fresh/server.ts";

interface HeaderProps {
  title: string;
  subtitle?: string;
}

/**
 * Header - A static server component
 * 
 * This component renders on the server only.
 * No JavaScript is shipped for this component.
 */
export default function Header({ title, subtitle }: HeaderProps) {
  // Static rendering - no interactivity needed
  const year = new Date().getFullYear();
  
  return (
    <header class="site-header">
      <div class="header-content">
        <div class="logo">
          <a href="/">
            <span class="logo-icon">⚡</span>
            <span class="logo-text">{title}</span>
          </a>
        </div>
        
        <nav class="main-nav">
          <ul>
            <li><a href="/">Home</a></li>
            <li><a href="/blog">Blog</a></li>
            <li><a href="/about">About</a></li>
          </ul>
        </nav>
        
        <div class="header-actions">
          {subtitle && <span class="subtitle">{subtitle}</span>}
        </div>
      </div>
      
      <div class="header-banner">
        <span class="runtime-badge">
          ⚡ Powered by runts &amp; Rust
        </span>
        {IS_BROWSER && (
          <span class="browser-badge">
            Client-side JavaScript active
          </span>
        )}
      </div>
    </header>
  );
}
