const url = new URL("\\\\x\\hello", "http://example.org/foo/bar");
if (url.href !== "http://x/hello") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL backslash network paths resolve authorities");
