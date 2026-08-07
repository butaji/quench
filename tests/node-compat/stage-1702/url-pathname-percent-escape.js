const url = new URL("/a%2fc", "http://example.org/foo/bar");
if (url.href !== "http://example.org/a%2fc") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL pathnames preserve percent escapes");
