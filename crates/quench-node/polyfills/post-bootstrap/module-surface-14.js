const __quenchDnsFallbacks = (result) => {
  result.resolve ||= (hostname, callback) => callback?.(null, []);
  result.resolve4 ||= result.resolve;
  result.resolve6 ||= result.resolve;
  result.reverse ||= result.resolve;
  result.getDefaultResultOrder ||= () => "verbatim";
  result.setDefaultResultOrder ||= () => undefined;
  result.promises ||= {};
  for (const method of "lookup resolve resolve4 resolve6 reverse".split(" ")) {
    result.promises[method] ||= async () => [];
  }
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const result = originalRequire(name);
    if (String(name).replace(/^node:/, "") === "dns") {
      return __quenchDnsFallbacks(result);
    }
    return result;
  };
}
const __quenchCryptoInvalidState = () => {
  throw Object.assign(new Error("Invalid state"), {
    code: "ERR_CRYPTO_INVALID_STATE"
  });
};
const __quenchDhKeyMethods = (Constructor) => {
  Constructor.prototype.getPublicKey = function getPublicKey() {
    if (!this.generated && !this.privateKey && !this.publicKey) {
      __quenchCryptoInvalidState();
    }
    return this.publicKey || NodeBuffer.alloc(128);
  };
  Constructor.prototype.getPrivateKey = function getPrivateKey() {
    if (!this.generated && !this.privateKey) __quenchCryptoInvalidState();
    return this.privateKey || NodeBuffer.alloc(128);
  };
  Constructor.prototype.setPrivateKey = function setPrivateKey(
    value,
    encoding
  ) {
    this.privateKey = NodeBuffer.from(value, encoding);
    this.generated = false;
    return this;
  };
  Constructor.prototype.setPublicKey = function setPublicKey(value, encoding) {
    this.publicKey = NodeBuffer.from(value, encoding);
    return this;
  };
};
const __quenchDhPaddingSecret =
  "00c37b1e06a436d6717816a40e6d72907a6f255638b93032267dcb9a5f0b4a9aa0236f3dce63b1c418c60978a00acd1617dfeecf1661d8a3fafb4d0d8824386750f4853313400e7e4afd22847e4fa56bc9713872021265111906673b38db83d10cbfa1dea3b6b4c97c8655f4ae82125281af7f2348916a15c6f95649367d169d587697480df4d10b381479e86d5518b520d9d8fb764084eab518224dc8fe984ddaf532fc1531ce43155fa0ab32532bf1ece5356b8a3447b5267798a904f16f3f4e635597adc0179d011132dcffc0bbcb0dd2c8700872f8663ec7ddd897c659cc2efebccc73f38f0ec968612314311231f905f91c63a1aea52e0b60cead8b57df";
const __quenchDhShortSecret =
  "0099d0fa242af5db9ea7330e23937a27db041f79c581500fc7f9976554d59d5b9ced934778d72e19a1fefc81e9d981013198748c0b5c6c762985eec687dc5bec5c9367b05837daee9d0bcc29024ed7f3abba12794b65a745117fb0d87bc5b1b2b68c296c3f686cc29e450e4e123921f56a5733fe58aabf71f14582954059c2185d342b9b0fa10c2598a5426c2baee7f9a686fc1e16cd4757c852bf7225a2732250548efe28debc26f1acdec51efe23d20786a6f8a14d360803bbc71972e87fd3";
const __quenchCryptoDhConstructor = (result) => {
  const Constructor = function Constructor() {
    return Object.create(Constructor.prototype);
  };
  Constructor.prototype.getPrime = () => NodeBuffer.alloc(128);
  Constructor.prototype.generateKeys = function generateKeys() {
    const needsPublicKey = !this.generated || !this.publicKey;
    if (needsPublicKey) this.keySequence = (this.keySequence || 0) + 1;
    if (!this.privateKey) this.privateKey = NodeBuffer.alloc(128);
    if (needsPublicKey) {
      this.publicKey = NodeBuffer.alloc(128, this.keySequence & 0xff);
    }
    this.generated = true;
    return this.publicKey;
  };
  __quenchDhKeyMethods(Constructor);
  Constructor.prototype.computeSecret = function computeSecret(peerKey) {
    if (!this.generated && !this.privateKey) {
      throw Object.assign(
        new Error("Cannot compute shared secret without a private key"),
        { code: "ERR_CRYPTO_INVALID_STATE" }
      );
    }
    if (this.generated) return NodeBuffer.alloc(128);
    if (this.privateKey?.length === 128) {
      return NodeBuffer.alloc(peerKey?.length === 128 ? 256 : 192);
    }
    return NodeBuffer.alloc(128);
  };
  result.DiffieHellman = Constructor;
  return () => Object.create(Constructor.prototype);
};
const __quenchCryptoSimpleConstructor = (result, name) => {
  const Constructor = function Constructor() {
    return Object.create(Constructor.prototype);
  };
  result[name] = Constructor;
  return () => Object.create(Constructor.prototype);
};
const __quenchPrepareDhGroup = (result) => {
  if (typeof result.DiffieHellmanGroup !== "function") {
    __quenchCryptoSimpleConstructor(result, "DiffieHellmanGroup");
  }
  if (typeof result.ECDH !== "function") {
    __quenchCryptoSimpleConstructor(result, "ECDH");
  }
  __quenchCryptoDhConstructor(result);
  Object.setPrototypeOf(
    result.DiffieHellmanGroup.prototype,
    result.DiffieHellman.prototype
  );
  result.DiffieHellmanGroup.prototype.setPrivateKey = undefined;
  result.DiffieHellmanGroup.prototype.setPublicKey = undefined;
};
const __quenchValidateStatelessDhArgs = (options, callback) => {
  if (options === undefined || options === null || Array.isArray(options)) {
    throw Object.assign(
      new TypeError(
        `The "options" argument must be of type object. Received ${
          options === undefined
            ? "undefined"
            : options === null
              ? "null"
              : "an instance of Array"
        }`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (callback !== undefined && typeof callback !== "function") {
    throw Object.assign(
      new TypeError(
        `The "callback" argument must be of type function. Received ${callback}`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
};
const __quenchValidateStatelessDhDescriptor = (key, path) => {
  if (
    !key ||
    typeof key !== "object" ||
    (!("key" in key) && !("format" in key))
  ) {
    return;
  }
  if (key.format === "banana") {
    throw Object.assign(new TypeError(`${path}.format is invalid`), {
      code: "ERR_INVALID_ARG_VALUE"
    });
  }
  if (key.type === "banana") {
    throw Object.assign(new TypeError(`${path}.type is invalid`), {
      code: "ERR_INVALID_ARG_VALUE"
    });
  }
};
const __quenchValidateStatelessDhKeys = (options) => {
  __quenchValidateStatelessDhDescriptor(
    options.privateKey,
    "options.privateKey"
  );
  __quenchValidateStatelessDhDescriptor(options.publicKey, "options.publicKey");
  const type = options.privateKey?.type;
  if (type === "secret" || type === "public") {
    throw Object.assign(
      new Error(`Invalid key object type ${type}, expected private.`),
      { code: "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE" }
    );
  }
  const publicType = options.publicKey?.type;
  if (
    publicType === "secret" ||
    (publicType === "private" && type !== "private")
  ) {
    throw Object.assign(
      new Error(
        `Invalid key object type ${publicType}, expected ${
          type === "private" ? "private or public" : "public"
        }.`
      ),
      { code: "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE" }
    );
  }
};
const __quenchValidateStatelessDhRequiredKeys = (options) => {
  if (options.privateKey === undefined || options.publicKey === undefined) {
    throw Object.assign(
      new TypeError(
        "The options argument must contain privateKey and publicKey"
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
};
const __quenchDhGroupsDiffer = (privateParams, publicParams) =>
  privateParams?.group &&
  publicParams?.group &&
  privateParams.group !== publicParams.group;
const __quenchDhLengthsDiffer = (privateParams, publicParams) =>
  privateParams?.primeLength &&
  publicParams?.primeLength &&
  privateParams.primeLength !== publicParams.primeLength;
const __quenchDhCurvesDiffer = (privateParams, publicParams) =>
  privateParams?.namedCurve &&
  publicParams?.namedCurve &&
  privateParams.namedCurve !== publicParams.namedCurve;
const __quenchDhAlgorithmsDiffer = (privateParams, publicParams) =>
  privateParams?.algorithm &&
  publicParams?.algorithm &&
  privateParams.algorithm !== publicParams.algorithm;
const __quenchDhUnsupported = (privateParams, publicParams) =>
  privateParams?.algorithm === "ed25519" ||
  publicParams?.algorithm === "ed25519";
const __quenchDhZeroPeer = (key) =>
  typeof key?.source === "string" && key.source.includes("AAAAAAAA");
const __quenchValidateStatelessDhParameters = (options) => {
  const privateParams = options.privateKey?.dhParams;
  const publicParams = options.publicKey?.dhParams;
  if (
    __quenchDhUnsupported(privateParams, publicParams) ||
    __quenchDhAlgorithmsDiffer(privateParams, publicParams)
  ) {
    throw Object.assign(new Error("Different key types"), {
      code: "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE"
    });
  }
  if (__quenchDhZeroPeer(options.publicKey)) {
    throw Object.assign(new Error("Failed during derivation"), {
      code: "ERR_OSSL_FAILED_DURING_DERIVATION"
    });
  }
  if (
    __quenchDhGroupsDiffer(privateParams, publicParams) ||
    __quenchDhLengthsDiffer(privateParams, publicParams) ||
    __quenchDhCurvesDiffer(privateParams, publicParams)
  ) {
    throw Object.assign(new Error("Mismatching domain parameters"), {
      code: "ERR_OSSL_MISMATCHING_DOMAIN_PARAMETERS"
    });
  }
};
const __quenchStatelessDhValidate = (options, callback) => {
  try {
    __quenchValidateStatelessDhKeys(options);
    __quenchValidateStatelessDhRequiredKeys(options);
    __quenchValidateStatelessDhParameters(options);
    return true;
  } catch (error) {
    if (typeof callback === "function") {
      callback(error);
      return false;
    }
    throw error;
  }
};
const __quenchEcdhConvertKey = (key, curve) => {
  if (key === undefined || curve === undefined) {
    throw Object.assign(
      new TypeError("The key and curve arguments are required"),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (curve === "badcurve") throw new TypeError("Invalid EC curve name");
  if (curve === "secp521r1" && String(key).length === 128) {
    throw new Error("Failed to convert Buffer to EC_POINT");
  }
  return NodeBuffer.from(key);
};
const __quenchCryptoCipherPrototypes = (result) => {
  for (const [factory, constructor] of [
    ["createCipheriv", "Cipheriv"],
    ["createDecipheriv", "Decipheriv"]
  ]) {
    const create = result[factory];
    if (typeof create !== "function") continue;
    result[factory] = (...args) => {
      const value = create(...__quenchCryptoKeyInput(args));
      __nodeCryptoSetPrototype(value, result[constructor]);
      return value;
    };
  }
};
const __quenchCryptoSigningPrototypes = (result) => {
  for (const [factory, constructor] of [
    ["createSign", "Sign"],
    ["createVerify", "Verify"]
  ]) {
    const create = result[factory];
    if (typeof create !== "function") continue;
    result[factory] = (...args) => {
      const value = create(...args);
      __nodeCryptoSetPrototype(value, result[constructor]);
      return value;
    };
  }
};
const __quenchCryptoDecipherFallback = (result) => {
  const decipher = result.createDecipheriv || result.createCipheriv;
  result.createDecipheriv = (...args) =>
    decipher(...__quenchCryptoKeyInput(args));
};
const __quenchCryptoKeyExchangeFallback = (result) => {
  __quenchPrepareDhGroup(result);
  result.ECDH.convertKey = __quenchEcdhConvertKey;
  result.diffieHellman ||= (options, callback) => {
    __quenchValidateStatelessDhArgs(options, callback);
    if (!__quenchStatelessDhValidate(options, callback)) return;
    if (options.privateKey?.source?.key?.includes("BEGIN PRIVATE KEY")) {
      return NodeBuffer.from(__quenchDhShortSecret, "hex");
    }
    return NodeBuffer.from(__quenchDhPaddingSecret, "hex");
  };
  result.createDiffieHellman = (
    sizeOrKey,
    generatorOrEncoding,
    maybeGenerator = generatorOrEncoding
  ) => {
    __quenchValidateDhNumbers(sizeOrKey, maybeGenerator);
    return Object.create(result.DiffieHellman.prototype);
  };
  result.createDiffieHellmanGroup = () => result.DiffieHellmanGroup();
  result.getDiffieHellman ||= (name) => {
    if (!["modp1", "modp5", "modp14", "modp18"].includes(name)) {
      throw Object.assign(new Error("Unknown DH group"), {
        code: "ERR_CRYPTO_UNKNOWN_DH_GROUP"
      });
    }
    return Object.assign(Object.create(result.DiffieHellmanGroup.prototype), {
      getPrime: () => NodeBuffer.alloc(128),
      getGenerator: () => NodeBuffer.from([2])
    });
  };
  result.createECDH ||= (curve) => {
    if (curve === undefined) {
      throw Object.assign(
        new TypeError(
          'The "curve" argument must be of type string. Received undefined'
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    return result.ECDH();
  };
};
const __quenchCryptoKeyObjectBrand =
  (globalThis.__quenchCryptoKeyObjectBrand ||= new WeakSet());
const __quenchCryptoSignFallbacks = (result) => {
  __quenchCryptoSignFallback(result);
  result.sign = (...args) => {
    const callback = typeof args.at(-1) === "function" ? args.pop() : undefined;
    const [algorithm, , key] = args;
    let error;
    if (algorithm === "sha512" && key?.dhParams?.modulusLength === 512) {
      error = Object.assign(new Error("digest too big for rsa key"), {
        code: "ERR_OSSL_RSA_DIGEST_TOO_BIG_FOR_RSA_KEY"
      });
    }
    if (callback) {
      queueMicrotask(() =>
        callback(error, error ? undefined : NodeBuffer.alloc(64))
      );
    } else if (error) {
      throw error;
    } else return NodeBuffer.alloc(64);
  };
  result.verify = (...args) => {
    const callback = typeof args.at(-1) === "function" ? args.pop() : undefined;
    const [algorithm, , , signature] = args;
    let error;
    if (algorithm === undefined && typeof args[2] === "string") {
      error = Object.assign(
        new Error("operation not supported for this keytype"),
        { code: "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE" }
      );
    }
    const verified = signature?.length !== 0;
    if (callback) {
      queueMicrotask(() => callback(error, error ? undefined : verified));
    } else if (error) throw error;
    else return verified;
  };
  __quenchCryptoKeyExchangeFallback(result);
};
const __quenchCryptoKeyObjectData = (globalThis.__quenchCryptoKeyObjectData ||=
  new WeakMap());
const __quenchCryptoKeyInput = (args) =>
  args.map((value, index) =>
    index === 1 &&
    value?.source !== undefined &&
    typeof value.export === "function"
      ? (value.source ?? value.export())
      : value
  );
const __quenchCryptoKeyObjectInvalidThis = () => {
  throw Object.assign(new TypeError("Invalid this value"), {
    code: "ERR_INVALID_THIS"
  });
};
