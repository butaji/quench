{
  if (globalThis.process) globalThis.process.emitWarning ||= () => undefined;
}
