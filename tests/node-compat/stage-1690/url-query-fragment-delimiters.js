const url = new URL("http://foo/path;a??e#f#g");
if (url.href !== "http://foo/path;a??e#f#g") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL query and fragment delimiters are preserved");
