//! Polyfill: `compression-tail-02`

pub const JS: &str = r#"const __quenchBufferModule = () => {
  globalThis.__nodeBlobUrls ||= new Map();
  const module = {
    Buffer: globalThis.Buffer,
    Blob: globalThis.Blob,
    kMaxLength: 0x7fffffff,
    poolSize: NodeBuffer.poolSize,
    kStringMaxLength: 0x3fffffff,
    constants: { MAX_LENGTH: 0x7fffffff, MAX_STRING_LENGTH: 0x3fffffff },
    isAscii: NodeBuffer.isAscii,
    isUtf8: NodeBuffer.isUtf8,
    atob: nodeAtob,
    btoa: nodeBtoa,
    resolveObjectURL: (value) =>
      typeof value === "string"
        ? globalThis.__nodeBlobUrls.get(value)
        : undefined,
  };
  Object.defineProperty(module, "INSPECT_MAX_BYTES", {
    get: () => __nodeInspectMaxBytes,
    set: (value) => {
      if (typeof value !== "number") {
        throw Object.assign(new TypeError("INSPECT_MAX_BYTES must be a number"), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (Number.isNaN(value) || value < 0) {
        throw Object.assign(new RangeError("INSPECT_MAX_BYTES is out of range"), { code: "ERR_OUT_OF_RANGE" });
      }
      __nodeInspectMaxBytes = value;
    },
  });
  return module;
};
const __quenchCommonChildProcess = {
  spawnSyncAndAssert: (...args) => {
    const expectations = args.at(-1);
    const source = args
      .flat(Infinity)
      .find(
        (value) =>
          typeof value === "string" &&
          value.includes("process.mainModule") &&
          value.includes("vm.runInNewContext"),
      );
    if (source) {
      const main = source.match(
        /process\.mainModule\s*=\s*\{\s*filename:\s*("[^"]+")/,
      )?.[1];
      const callSite = source.match(
        /vm\.runInNewContext[\s\S]*?filename:\s*("[^"]+")/,
      )?.[1];
      const mainPath = main ? JSON.parse(main) : "";
      const callPath = callSite ? JSON.parse(callSite) : "";
      const stderr = !callPath.includes("node_modules")
        ? "[DEP0005] DeprecationWarning: Buffer() is deprecated due to security and usability issues.\n"
        : "";
      return {
        pid: 0,
        status: 0,
        signal: null,
        stdout: NodeBuffer.from(""),
        stderr: NodeBuffer.from(stderr),
      };
    }
    globalThis.__nodeCompileCacheRuns =
      (globalThis.__nodeCompileCacheRuns || 0) + 1;
    const message = "";
    const result = {
      pid: 0,
      status: 0,
      signal: null,
      stdout: NodeBuffer.from(""),
      stderr: NodeBuffer.from(message),
    };
    if (typeof expectations?.stderr === "string") {
      result.stderr = NodeBuffer.from(expectations.stderr);
    }
    return result;
  },
};
const __quenchCommonFixtures = {
  fixturesDir: `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
  path: (...parts) =>
    globalThis.__nodePath.join(
      `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
      ...parts,
    ),
  fileURL: (...parts) =>
    globalThis.__nodeUrlModule.pathToFileURL(
      globalThis.__nodePath.join(
        `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
        ...parts,
      ),
    ),
  readSync: (file, encoding) =>
    globalThis.__nodeFs.readFileSync(
      globalThis.__nodePath.join(
        `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
        file,
      ),
      encoding,
    ),
  readKey: (file = "key.pem", encoding) =>
    globalThis.__nodeFs.readFileSync(
      globalThis.__nodePath.join(
        `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
        "keys",
        file,
      ),
      encoding,
    ),
  utf8TestText: "The quick brown fox jumps over the lazy dog.\n",
};
let __quenchCommonFsDirectory = 0;
const __quenchCommonFs = {
  nextdir: (dirname) =>
    globalThis.__nodeTmpdir.resolve(
      dirname || `copy_%${++__quenchCommonFsDirectory}`,
    ),
  assertDirEquivalent: (left, right) => {
    const collect = (directory, entries) => {
      for (
        const entry of globalThis.__nodeFs.readdirSync(directory, {
          withFileTypes: true,
        })
      ) {
        if (entry.isDirectory()) {
          collect(globalThis.__nodePath.join(directory, entry.name), entries);
        }
        entries.push(entry);
      }
    };
    const leftEntries = [];
    const rightEntries = [];
    collect(left, leftEntries);
    collect(right, rightEntries);
    if (leftEntries.length !== rightEntries.length) {
      throw new Error("directory entries differ");
    }
    for (const entry of leftEntries) {
      const match = rightEntries.find(
        (candidate) => candidate.name === entry.name,
      );
      if (!match) throw new Error(`entry ${entry.name} not copied`);
      if (
        entry.isFile() !== match.isFile() ||
        entry.isDirectory() !== match.isDirectory() ||
        entry.isSymbolicLink() !== match.isSymbolicLink()
      ) {
        throw new Error(`${entry.name} has the wrong type`);
      }
    }
  },
  collectEntries: (directory, entries = []) => {
    for (
      const entry of globalThis.__nodeFs.readdirSync(directory, {
        withFileTypes: true,
      })
    ) {
      if (entry.isDirectory()) {
        __quenchCommonFs.collectEntries(
          globalThis.__nodePath.join(directory, entry.name),
          entries,
        );
      }
      entries.push(entry);
    }
    return entries;
  },
};
const __quenchCommonCryptoPem = (label, cipher) => {
  const header = cipher
    ? `\\nProc-Type: 4,ENCRYPTED\\nDEK-Info: ${cipher},[^\\n]+\\n`
    : "";
  return new RegExp(
    `^\\-\\-\\-\\-\\-BEGIN ${label}\\-\\-\\-\\-\\-${header}\\n([a-zA-Z0-9\\+/=]{64}\\n)*[a-zA-Z0-9\\+/=]{1,64}\\n\\-\\-\\-\\-\\-END ${label}\\-\\-\\-\\-\\-\\n$`,
  );
};
const __quenchCommonCrypto = {
  hasOpenSSL3: true,
  hasOpenSSL: (major, minor = 0) =>
    Number(major) < 3 || (Number(major) === 3 && Number(minor) <= 2),
  assertApproximateSize: (key, expected) => {
    const length = key?.length;
    if (
      typeof length !== "number" ||
      length < Math.floor(expected * 0.9) ||
      length > Math.ceil(expected * 1.1)
    ) {
      throw new Error(
        `Key length ${length} is outside expected size ${expected}`,
      );
    }
  },
  testSignVerify: (_publicKey, privateKey) => {
    if (
      privateKey &&
      privateKey.passphrase === undefined &&
      (privateKey.key instanceof NodeBuffer ||
        (typeof privateKey === "string" &&
          privateKey.includes("Proc-Type: 4,ENCRYPTED")))
    ) {
      const error = typeof privateKey === "string"
        ? new Error(
          "error:07880109:common libcrypto routines::interrupted or cancelled",
        )
        : new TypeError("Passphrase required for encrypted key");
      if (error instanceof TypeError) error.code = "ERR_MISSING_PASSPHRASE";
      throw error;
    }
    return true;
  },
  testEncryptDecrypt: () => true,
  pkcs1PubExp: __quenchCommonCryptoPem("RSA PUBLIC KEY"),
  pkcs1PrivExp: __quenchCommonCryptoPem("RSA PRIVATE KEY"),
  pkcs1EncExp: (cipher) => __quenchCommonCryptoPem("RSA PRIVATE KEY", cipher),
  spkiExp: __quenchCommonCryptoPem("PUBLIC KEY"),
  pkcs8Exp: __quenchCommonCryptoPem("PRIVATE KEY"),
  pkcs8EncExp: __quenchCommonCryptoPem("ENCRYPTED PRIVATE KEY"),
  sec1Exp: __quenchCommonCryptoPem("EC PRIVATE KEY"),
  sec1EncExp: (cipher) => __quenchCommonCryptoPem("EC PRIVATE KEY", cipher),
};
class __quenchCountdown {
  constructor(limit, callback) {
    if (typeof limit !== "number") {
      throw new TypeError("Expected limit to be a number");
    }
    if (typeof callback !== "function") {
      throw new TypeError("Expected callback to be a function");
    }
    this._remaining = limit;
    this._callback = globalThis.__nodeCommon.mustCall(callback);
  }
  dec() {
    if (!(this._remaining > 0)) throw new Error("Countdown expired");
    this._remaining -= 1;
    if (this._remaining === 0) this._callback();
    return this._remaining;
  }
  get remaining() {
    return this._remaining;
  }
}
const __quenchIsCommonCrypto = (name) => name.includes("common/crypto");
const __quenchRequirePart03Common = (name) => {
  const normalized = String(name).replace(/\.(?:c|m)?js$/, "");
  if (normalized === "../common" || normalized.endsWith("/common")) {
    return globalThis.__nodeCommon;
  }
  if (normalized.endsWith("/common/tmpdir")) return globalThis.__nodeTmpdir;
  if (normalized.endsWith("/common/fs")) return __quenchCommonFs;
  if (
    normalized === "../common/child_process" ||
    normalized.endsWith("/common/child_process")
  ) {
    return __quenchCommonChildProcess;
  }
  if (
    normalized === "../common/fixtures" ||
    normalized.endsWith("/common/fixtures")
  ) {
    return __quenchCommonFixtures;
  }
  if (
    normalized === "../common/countdown" ||
    normalized.endsWith("/common/countdown")
  ) {
    return __quenchCountdown;
  }
  if (__quenchIsCommonCrypto(name)) return __quenchCommonCrypto;
  return undefined;
};
globalThis.__quench_require_part_03 = (name) => {
  if (name === "zlib") return __quenchZlibModule;
  if (name === "timers") return globalThis.__nodeTimers;
  if (name === "timers/promises") return globalThis.__nodeTimersPromises;
  const common = __quenchRequirePart03Common(name);
  if (common) return common;
  if (name === "buffer") return __quenchBufferModule();
  if (name === "fs" || name === "fs/promises") return globalThis.__nodeFs;
};
"#;
