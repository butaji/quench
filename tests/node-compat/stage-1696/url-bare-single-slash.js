const url = new URL("http:/", "http://example.com/");
if (url.href !== "http://example.com/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL bare single-slash schemes resolve to the base root");
