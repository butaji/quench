{
  if (globalThis.process) {
    const report = (globalThis.process.report ||= {});
    report.compact ??= false;
    report.directory ??= "";
    report.excludeEnv ??= false;
    report.excludeNetwork ??= false;
    report.filename ??= "";
    report.reportOnFatalError ??= false;
    report.reportOnSignal ??= false;
    report.reportOnUncaughtException ??= false;
    report.signal ??= "SIGUSR2";
    report.getReport ||= () => ({});
    report.writeReport ||= () => undefined;
  }
}
