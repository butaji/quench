// Routes module
//! Defines the application routes and handlers

use axum::{
    Router,
    routing::get,
    response::Html,
    extract::Path,
};
use serde_json::json;

/// Home page
pub async fn index_handler() -> Html<String> {
    Html(r#"<!DOCTYPE html>
<html>
<head>
    <title>runts Blog</title>
    <style>
        body { font-family: system-ui; margin: 0; padding: 20px; background: #f5f5f5; }
        .container { max-width: 800px; margin: 0 auto; }
        .hero { background: white; padding: 40px; border-radius: 8px; margin-bottom: 20px; }
        .hero h1 { margin: 0 0 10px 0; color: #333; }
        .hero p { color: #666; margin: 0; }
        .demo { background: white; padding: 20px; border-radius: 8px; }
        .demo h2 { margin-top: 0; }
        .btn { display: inline-block; padding: 10px 20px; background: #0070f3; color: white; 
               text-decoration: none; border-radius: 4px; margin-right: 10px; }
        .btn:hover { background: #0051cc; }
        footer { text-align: center; color: #666; margin-top: 40px; }
    </style>
</head>
<body>
    <div class="container">
        <div class="hero">
            <h1>Welcome to runts!</h1>
            <p>Build lightning-fast web apps with Fresh/Preact and native Rust</p>
            <p>Version: 0.2.0</p>
            <div style="margin-top: 20px;">
                <a href="/blog" class="btn">Read the Blog</a>
                <a href="/about" class="btn">Learn More</a>
            </div>
        </div>
        
        <div class="demo">
            <h2>Interactive Demo</h2>
            <p>Counter Island will be loaded here:</p>
            <div id="counter-island">
                <!-- Counter island rendered here -->
                <button onclick="alert('Island would hydrate here in full implementation')">
                    Counter: 0 (Click me)
                </button>
            </div>
        </div>
        
        <footer>
            <p>Built with runts and Rust</p>
        </footer>
    </div>
</body>
</html>"#.to_string())
}

/// Blog listing page
pub async fn blog_index_handler() -> Html<String> {
    let posts = vec![
        json!({
            "slug": "hello-world",
            "title": "Getting Started with runts",
            "excerpt": "Learn how to build fast web apps with native Rust"
        }),
        json!({
            "slug": "islands-architecture",
            "title": "Understanding Islands Architecture",
            "excerpt": "How partial hydration enables blazing fast pages"
        }),
    ];
    
    let posts_html: String = posts.iter().map(|p| {
        format!(
            r#"<article class="post-preview">
                <h2><a href="/blog/{}">{}</a></h2>
                <p>{}</p>
            </article>"#,
            p["slug"], p["title"], p["excerpt"]
        )
    }).collect();
    
    Html(format!(r#"<!DOCTYPE html>
<html>
<head>
    <title>Blog - runts</title>
    <style>
        body {{ font-family: system-ui; margin: 0; padding: 20px; background: #f5f5f5; }}
        .container {{ max-width: 800px; margin: 0 auto; }}
        .header {{ background: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }}
        .header h1 {{ margin: 0; color: #333; }}
        .post-preview {{ background: white; padding: 20px; border-radius: 8px; margin-bottom: 15px; }}
        .post-preview h2 {{ margin: 0 0 10px 0; }}
        .post-preview h2 a {{ color: #0070f3; text-decoration: none; }}
        .post-preview p {{ color: #666; margin: 0; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Blog</h1>
            <p>Total posts: {}</p>
        </div>
        <main class="blog-list">
            {}
        </main>
    </div>
</body>
</html>"#, posts.len(), posts_html))
}

/// Individual blog post
pub async fn blog_slug_handler(Path(slug): Path<String>) -> Html<String> {
    // In a real app, this would fetch from a database
    Html(format!(r#"<!DOCTYPE html>
<html>
<head>
    <title>{} - runts Blog</title>
    <style>
        body {{ font-family: system-ui; margin: 0; padding: 20px; background: #f5f5f5; }}
        .container {{ max-width: 800px; margin: 0 auto; }}
        article {{ background: white; padding: 40px; border-radius: 8px; }}
        h1 {{ margin: 0 0 10px 0; color: #333; }}
        .meta {{ color: #666; margin-bottom: 20px; }}
        .content {{ line-height: 1.6; }}
        a {{ color: #0070f3; }}
    </style>
</head>
<body>
    <div class="container">
        <article>
            <h1>{}</h1>
            <p class="meta">By Author on 2026-01-01</p>
            <div class="content">
                <p>This is the content for the blog post "{}".</p>
                <p>In a full implementation, this would be loaded from a database or CMS.</p>
            </div>
            <p><a href="/blog">Back to blog</a></p>
        </article>
    </div>
</body>
</html>"#, slug, slug, slug))
}

/// Build the application router
pub fn build_router() -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/blog", get(blog_index_handler))
        .route("/blog/:slug", get(blog_slug_handler))
}
