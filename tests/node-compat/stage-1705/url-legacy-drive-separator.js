const url = new URL("  File:c|////foo\\bar.html", "file:///tmp/mock/path");
if (url.href !== "file:///c:////foo/bar.html") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL legacy drive separators are normalized");
