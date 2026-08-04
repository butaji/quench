{
  if (globalThis.process) {
    globalThis.process.getActiveResourcesInfo = () => [];
    globalThis.process.availableMemory = () => Number.MAX_SAFE_INTEGER;
    globalThis.process.setSourceMapsEnabled = () => undefined;
    globalThis.process.sourceMapsEnabled = false;
    globalThis.process.debugPort = 9229;
    globalThis.process.release = {
      name: "node",
      sourceUrl: "",
      headersUrl: ""
    };
    globalThis.process.allowedNodeEnvironmentFlags = new Set();
    globalThis.process.execArgv = [];
    globalThis.process.argv0 = "node";
    globalThis.process.features ||= {};
    globalThis.process.features.inspector ??= false;
    globalThis.process.noDeprecation ??= false;
    globalThis.process.traceDeprecation ??= false;
    globalThis.process.throwDeprecation ??= false;
    globalThis.process.version ||= "v22.0.0";
    globalThis.process.versions ||= {};
    globalThis.process.versions.node ??= "22.0.0";
    globalThis.process.versions.v8 ??= "12.4.254.21-node.20";
    globalThis.process.versions.uv ??= "1.48.0";
    globalThis.process.versions.openssl ??= "3.0.13";
    globalThis.process.versions.zlib ??= "1.3.0";
    globalThis.process.versions.modules ??= "127";
    globalThis.process.versions.napi ??= "9";
    globalThis.process.versions.acorn ??= "8.11.3";
    globalThis.process.versions.ada ??= "2.7.8";
    globalThis.process.versions.tz ??= "2024a";
    globalThis.process.versions.brotli ??= "1.1.0";
    globalThis.process.versions.nbytes ??= "1.0.0";
    globalThis.process.versions.cldr ??= "45.0";
    globalThis.process.versions.icu ??= "75.1";
    globalThis.process.versions.nghttp2 ??= "1.61.0";
    globalThis.process.versions.llhttp ??= "9.2.1";
    globalThis.process.versions.nghttp3 ??= "1.3.0";
    globalThis.process.versions.ngtcp2 ??= "1.4.0";
    globalThis.process.versions.simdutf ??= "5.2.4";
    globalThis.process.versions.unicode ??= "15.1";
    globalThis.process.versions.undici ??= "6.19.8";
    globalThis.process.versions.cjs_module_lexer ??= "1.2.2";
    globalThis.process.title ||= "node";
    globalThis.process.getBuiltinModule ||= (name) =>
      globalThis.require(String(name).replace(/^node:/, ""));
    globalThis.process.loadEnvFile ||= () => undefined;
    globalThis.process.finalization ||= {
      register: () => undefined,
      unregister: () => undefined,
      registerBeforeExit: () => undefined
    };
    globalThis.process.permission ||= { has: () => false };
    globalThis.process.resourceUsage ||= () => ({
      userCPUTime: 0,
      systemCPUTime: 0,
      maxRSS: 0,
      minorPageFault: 0,
      majorPageFault: 0,
      fsRead: 0,
      fsWrite: 0,
      involuntaryContextSwitches: 0,
      voluntaryContextSwitches: 0
    });
    globalThis.process.cpuUsage ||= () => ({ user: 0, system: 0 });
    globalThis.process.memoryUsage.rss ||= () => 0;
  }
}
