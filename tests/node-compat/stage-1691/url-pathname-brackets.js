const url = new URL("[61:24:74]:98", "http://example.org/foo/bar");
if (url.href !== "http://example.org/foo/[61:24:74]:98") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL pathnames preserve brackets");
