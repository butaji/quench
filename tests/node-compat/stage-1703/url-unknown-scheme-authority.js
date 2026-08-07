const url = new URL("ssh://example.com/foo/bar.git");
if (url.href !== "ssh://example.com/foo/bar.git") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("Unknown URL schemes preserve authority hosts");
