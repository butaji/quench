// components/Header.tsx - Static header component
import { IS_BROWSER } from "$fresh/server.ts";

interface HeaderProps {
  title: string;
  subtitle?: string;
}

export default function Header({ title, subtitle }: HeaderProps) {
  const year = 2026;
  
  return (
    <header class="site-header">
      <div class="header-content">
        <div class="logo">
          <a href="/">
            <span class="logo-icon">runts</span>
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
          <span class="subtitle">{subtitle}</span>
        </div>
      </div>
      
      <div class="header-banner">
        <span class="runtime-badge">
          Powered by runts and Rust
        </span>
      </div>
    </header>
  );
}
