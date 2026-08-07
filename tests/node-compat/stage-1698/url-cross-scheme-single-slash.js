const url = new URL("https:/example.com/", "http://example.org/foo/bar");
if (url.href !== "https://example.com/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL cross-scheme single-slash references remain absolute");
