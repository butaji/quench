if (process.stdout.isTTY !== false || process.stderr.isTTY !== false) {
  throw new Error("process stream flags");
}
