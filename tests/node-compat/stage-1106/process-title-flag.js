// Flags: --title=stage-title
if (process.title !== "stage-title") {
  throw new Error(`unexpected process title: ${process.title}`);
}
