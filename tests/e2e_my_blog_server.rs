//! End-to-end test: build the `examples/my-blog` project with the
//! fresh plugin, boot the resulting server, and curl every route.
//!
//! What this test verifies:
//! - The pipeline `TSX → HIR → Rust → cargo build --release →
//!   binary` produces a working executable.
//! - The server actually starts on port 8000 and responds to
//!   HTTP GET requests on all four routes
//!   (`/`, `/about`, `/blog`, `/blog/:slug`).
//! - The response body for each route is a non-empty HTML
//!   snippet that contains the component class the user wrote
//!   in the corresponding `.tsx` file.
//!
//! This test is the highest-level proof that the project is
//! feature-complete end-to-end. If this passes, the
//! `.tsx → native binary → HTTP server` pipeline is wired up
//! correctly for the my-blog example.

use std::io::Read;
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Find a `runts` binary we can use to drive the build.
///
/// The test runner has `$CARGO_BIN_EXE_runts` set to the path of
/// the runts binary that the test was built against, so we use
/// that. We deliberately don't fall back to `$PATH` lookup
/// because that would introduce a `which` dev-dep for a code
/// path that should never be hit in practice.
fn find_runts_binary() -> Option<std::path::PathBuf> {
    let p = option_env!("CARGO_BIN_EXE_runts")?;
    Some(std::path::PathBuf::from(p))
}

/// Issue a minimal HTTP/1.0 GET against `addr + path` and
/// return the body bytes. We use raw TCP because the test
/// does not want to take a hard dep on a real HTTP client.
/// The 1.0 version skips keep-alive so the connection
/// closes after the response, simplifying shutdown.
fn http_get(addr: std::net::SocketAddr, path: &str) -> (u16, String) {
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(5)).expect("TCP connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set_read_timeout");
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nUser-Agent: e2e_my_blog_server\r\n\r\n"
    );
    use std::io::Write;
    stream.write_all(req.as_bytes()).expect("write_all");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).expect("read_to_end");
    let raw = String::from_utf8_lossy(&buf).to_string();
    let mut parts = raw.splitn(2, "\r\n\r\n");
    let head = parts.next().unwrap_or("");
    let body = parts.next().unwrap_or("").to_string();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    (status, body)
}

/// Issue a minimal HTTP/1.0 GET and return (status, headers, body).
/// The headers vec contains `(name, value)` pairs in
/// insertion order, with the header name lowercased.
fn http_get_with_headers(
    addr: std::net::SocketAddr,
    path: &str,
) -> (u16, Vec<(String, String)>, String) {
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(5)).expect("TCP connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set_read_timeout");
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nUser-Agent: e2e_my_blog_server\r\n\r\n"
    );
    use std::io::Write;
    stream.write_all(req.as_bytes()).expect("write_all");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).expect("read_to_end");
    let raw = String::from_utf8_lossy(&buf).to_string();
    let mut parts = raw.splitn(2, "\r\n\r\n");
    let head = parts.next().unwrap_or("");
    let body = parts.next().unwrap_or("").to_string();
    let mut status = 0;
    let mut headers: Vec<(String, String)> = Vec::new();
    for (i, line) in head.lines().enumerate() {
        if i == 0 {
            // HTTP/1.x STATUS REASON
            status = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(0);
        } else if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_lowercase(), value.trim().to_string()));
        }
    }
    (status, headers, body)
}

/// Wait for the server to start accepting connections, up to
/// `timeout`. We poll the TCP port: any successful connect means
/// the listener is up.
fn wait_for_listen(addr: std::net::SocketAddr, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

struct RunningServer {
    child: Child,
    port: u16,
}

impl RunningServer {
    fn addr(&self) -> std::net::SocketAddr {
        std::net::SocketAddr::from(([127, 0, 0, 1], self.port))
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Boot the my-blog server on port 8000 and return a guard
/// that kills the child on drop.
///
/// The `runts-app` binary currently hard-codes port 8000, so
/// the caller is responsible for ensuring the port is free
/// before running the test. The test marker is `#[ignore]` by
/// default to avoid network requirements on CI; pass
/// `--ignored` to opt in.
fn boot_my_blog() -> Option<RunningServer> {
    let bin = find_runts_binary()?;
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let example = manifest_dir.join("examples").join("my-blog");
    if !example.exists() {
        eprintln!(
            "skipping: my-blog example not found at {}",
            example.display()
        );
        return None;
    }

    // Build via the runts CLI so the test exercises the same
    // code path users hit on the command line.
    let status = Command::new(&bin)
        .arg("build")
        .arg(&example)
        .arg("--plugin")
        .arg("fresh")
        .status()
        .expect("spawn runts build");
    if !status.success() {
        panic!("`runts build examples/my-blog --plugin fresh` failed");
    }

    let binary = example.join("target").join("release").join("runts-app");
    if !binary.exists() {
        panic!("expected binary at {}", binary.display());
    }

    let mut child = Command::new(&binary)
        .current_dir(&example)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn runts-app");

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8000));
    if !wait_for_listen(addr, Duration::from_secs(15)) {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start listening on {} within 15s", addr);
    }

    Some(RunningServer { child, port: 8000 })
}

#[test]
#[ignore = "boots a real HTTP server on port 8000; run with --ignored"]
fn my_blog_all_routes_serve_per_component_html() {
    let Some(server) = boot_my_blog() else {
        return; // example missing
    };
    let addr = server.addr();

    // (path, expected substring in the body)
    let cases: &[(&str, &str)] = &[
        ("/", "class=\"home\""),
        ("/about", "class=\"about-page\""),
        ("/blog", "class=\"blog-index\""),
        ("/blog/introducing-runts", "class=\"not-found\""),
    ];

    for (path, expected) in cases {
        let (status, body) = http_get(addr, path);
        assert_eq!(status, 200, "GET {path} returned {status}, body={body}");
        assert!(
            body.contains(expected),
            "GET {path} body should contain {expected:?}, got first 200 bytes: {}",
            &body[..body.len().min(200)]
        );
        assert!(
            !body.is_empty(),
            "GET {path} returned an empty body"
        );
    }
}

#[test]
fn my_blog_unknown_route_returns_404() {
    let Some(server) = boot_my_blog() else {
        return;
    };
    let addr = server.addr();
    let (status, body) = http_get(addr, "/this/route/does/not/exist");
    // axum's default NotFound returns 404 with an empty body.
    // The exact body depends on axum's defaults; we just
    // assert the status code.
    assert_eq!(
        status, 404,
        "GET unknown path should return 404, got {status}, body={body}"
    );
}

#[test]
fn my_blog_about_route_includes_route_html() {
    // Sanity check: the about page has the structure the
    // user wrote in `routes/about.tsx`. If the codegen ever
    // drops the inner content, this test catches it.
    let Some(server) = boot_my_blog() else {
        return;
    };
    let addr = server.addr();
    let (status, body) = http_get(addr, "/about");
    assert_eq!(status, 200);
    // The about page should at least render the structural
    // shell (h1, h2, p, ul, nav) that the source defines.
    for marker in [
        "<div class=\"about-page\">",
        "<h1>",
        "<h2>Features</h2>",
        "<nav>",
    ] {
        assert!(
            body.contains(marker),
            "about page body should contain {marker:?}, got first 250 bytes: {}",
            &body[..body.len().min(250)]
        );
    }
}

/// The my-blog `routes/_middleware.ts` adds an
/// `X-Response-Time` header to every response. This test boots
/// the server, hits a couple of routes, and asserts the header
/// is present (a non-empty value containing `ms`).
#[test]
#[ignore]
fn my_blog_middleware_adds_x_response_time_header() {
    let Some(server) = boot_my_blog() else {
        return;
    };
    let addr = server.addr();
    for path in ["/", "/about", "/blog", "/blog/introducing-runts"] {
        let (status, headers, _body) = http_get_with_headers(addr, path);
        assert_eq!(status, 200, "GET {path} expected 200, got {status}");
        let header = headers
            .iter()
            .find(|(k, _)| k == "x-response-time")
            .map(|(_, v)| v.clone());
        let value = header.unwrap_or_else(|| {
            panic!(
                "GET {path} response missing X-Response-Time header; got: {headers:?}"
            )
        });
        assert!(
            value.ends_with("ms"),
            "X-Response-Time for {path} should end in 'ms', got {value:?}"
        );
    }
}

/// `routes/blog/[slug].tsx` defines a dynamic route at
/// `/blog/:slug`. The handler's signature should be
/// `Path(slug): Path<String>`, which means the route matches
/// for any URL under `/blog/...`. This test boots the server
/// and verifies that three different slugs each return HTTP 200
/// (with the per-page render body), and that a non-matching
/// path (e.g. `/articles/...`) still 404s.
#[test]
#[ignore]
fn my_blog_dynamic_route_matches_arbitrary_slugs() {
    let Some(server) = boot_my_blog() else {
        return;
    };
    let addr = server.addr();
    for slug in ["introducing-runts", "anything-goes-here", "x"] {
        let path = format!("/blog/{slug}");
        let (status, _body) = http_get(addr, &path);
        assert_eq!(status, 200, "GET {path} expected 200, got {status}");
    }
    // Sanity: a path that doesn't match any route should 404.
    let (status, _body) = http_get(addr, "/articles/something");
    assert_eq!(
        status, 404,
        "GET /articles/something expected 404, got {status}"
    );
}

/// The slug that reaches a dynamic-route handler should
/// actually appear in the response body. Two codegen
/// paths both surface the slug:
///
///   * The JSX-codegen path wraps the inner JSX in a
///     `<div data-route-param="<slug>">` so the value is
///     visible as an HTML attribute.
///   * The stub-codegen path (used for routes where
///     `try_codegen_jsx` returns None) renders a
///     `<p>route-param: <code>slug</code></p>` block.
///
/// We assert on the JSX-codegen marker (the attribute) since
/// that is what my-blog currently exercises. If the stub
/// path becomes the dominant codegen in the future, this
/// test can be extended to look for the `<p>`/`<code>`
/// markers too.
#[test]
#[ignore]
fn my_blog_dynamic_route_includes_slug_in_body() {
    let Some(server) = boot_my_blog() else {
        return;
    };
    let addr = server.addr();
    for slug in ["introducing-runts", "anything-here", "x"] {
        let path = format!("/blog/{slug}");
        let (status, body) = http_get(addr, &path);
        assert_eq!(status, 200, "GET {path} expected 200, got {status}");
        // The render emits a `data-route-param="<slug>"`
        // attribute on the wrapping div. This is the
        // JSX-codegen path's marker.
        assert!(
            body.contains(&format!("data-route-param=\"{slug}\"")),
            "GET {path} body should contain data-route-param=\"{slug}\", got first 300 bytes: {}",
            &body[..body.len().min(300)]
        );
    }
}
