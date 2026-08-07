{
  if (
    globalThis.process &&
    globalThis.process.allowedNodeEnvironmentFlags instanceof Set &&
    globalThis.process.allowedNodeEnvironmentFlags.size === 0
  ) {
    globalThis.process.allowedNodeEnvironmentFlags.add("--no-warnings");
  }
}
