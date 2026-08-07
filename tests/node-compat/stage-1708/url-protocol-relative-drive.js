const url = new URL("//C|/foo/bar", "file:///tmp/mock/path");
if (url.href !== "file:///C:/foo/bar") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log(
  "URL protocol-relative drive references resolve against file bases",
);
