const url = new URL("ftp:/example.com/", "http://example.org/foo/bar");
if (url.href !== "ftp://example.com/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL FTP single-slash references remain absolute");
