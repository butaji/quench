const url = new URL("#;?", "http://example.org/foo/bar");
if (url.href !== "http://example.org/foo/bar#;?") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL fragments preserve punctuation");
