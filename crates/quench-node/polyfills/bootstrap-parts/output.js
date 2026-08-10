const __quenchChildOutputRequire = globalThis.require;
const __quenchChildOutputModule = __quenchChildOutputRequire("child_process");
const __quenchOutputSpawn = __quenchChildOutputModule.spawn;
const __quenchOutputStream = (stream) => {
  stream._quenchListeners ||= {};
  stream.on = (event, listener) => {
    (stream._quenchListeners[event] ||= []).push(listener);
    return stream;
  };
  stream.once = (event, listener) => {
    const once = (...values) => {
      stream._quenchListeners[event] = (
        stream._quenchListeners[event] || []
      ).filter((item) => item !== once);
      listener(...values);
    };
    return stream.on(event, once);
  };
  stream.emit = (event, ...values) => {
    for (const listener of [...(stream._quenchListeners[event] || [])]) {
      listener(...values);
    }
    return true;
  };
  return stream;
};
const __quenchWriteChildOutput = (args, stdout, stderr) => {
  const options = Array.isArray(args[1]) ? args[2] || {} : args[1] || {};
  const env = String(args[0]).endsWith("/env") || String(args[0]) === "env"
    ? options.env === undefined ? process.env : options.env
    : {};
  const outputEntries = Object.entries(env).filter(
    ([, value]) => value !== undefined,
  );
  const output =
    outputEntries.map(([key, value]) => `${key}=${String(value)}`).join("\n") +
    (outputEntries.length ? "\n" : "");
  if (output) stdout.emit("data", output);
  stdout.emit("end");
  stderr.emit("end");
  stdout.emit("close");
  stderr.emit("close");
};
const __quenchEmitChildOutput = (
  child,
  emit,
  args,
  stdout,
  stderr,
  event,
  values,
) => {
  if (event === "exit" && !child.__quenchOutputSent) {
    child.__quenchOutputSent = true;
    if (!child.__spawnEmitted) {
      child.__spawnEmitted = true;
      emit.call(child, "spawn");
    }
    __quenchWriteChildOutput(args, stdout, stderr);
  }
  return emit.call(child, event, ...values);
};
__quenchChildOutputModule.spawn = (...args) => {
  const child = __quenchOutputSpawn(...args);
  const stdout = __quenchOutputStream(child.stdout);
  const stderr = __quenchOutputStream(child.stderr);
  const emit = child.emit;
  child.emit = (event, ...values) =>
    __quenchEmitChildOutput(child, emit, args, stdout, stderr, event, values);
  return child;
};
