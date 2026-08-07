const url = new URL(":foo.com\\", "http://example.org/foo/bar");
if (url.href !== "http://example.org/foo/:foo.com/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("Special URL paths normalize backslashes");
