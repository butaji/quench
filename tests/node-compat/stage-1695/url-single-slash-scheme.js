const url = new URL("http:/example.com/", "http://example.org/foo/bar");
if (url.href !== "http://example.org/example.com/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL single-slash special schemes resolve against the base");
