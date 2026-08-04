{
  if (globalThis.process) {
    const config = (globalThis.process.config ||= {});
    config.variables ||= {};
    config.target_defaults ||= {};
  }
}
