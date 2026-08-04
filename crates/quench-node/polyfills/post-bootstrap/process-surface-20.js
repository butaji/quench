{
  if (globalThis.process) {
    let captureCallback = null;
    globalThis.process.setUncaughtExceptionCaptureCallback ||= (callback) => {
      captureCallback = callback;
    };
    globalThis.process.hasUncaughtExceptionCaptureCallback ||= () =>
      captureCallback !== null;
  }
}
