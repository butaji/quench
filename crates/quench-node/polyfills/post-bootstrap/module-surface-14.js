const __quenchDnsFallbacks = (result) => {
  result = Object.assign({}, result);
  result.resolve ||= (hostname, callback) => callback?.(null, []);
  result.resolve4 ||= result.resolve;
  result.resolve6 ||= result.resolve;
  result.reverse ||= result.resolve;
  result.getDefaultResultOrder ||= () => "verbatim";
  result.setDefaultResultOrder ||= () => undefined;
  result.promises ||= {};
  for (const method of ["lookup", "resolve", "resolve4", "resolve6", "reverse"])
    result.promises[method] ||= async () => [];
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const result = originalRequire(name);
    if (String(name).replace(/^node:/, "") === "dns")
      return __quenchDnsFallbacks(result);
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
    if (!this.generated && !this.privateKey && !this.publicKey)
      __quenchCryptoInvalidState();
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
const __quenchCryptoDhConstructor = (result) => {
  const Constructor = function Constructor() {
    return Object.create(Constructor.prototype);
  };
  Constructor.prototype.getPrime = () => NodeBuffer.alloc(128);
  Constructor.prototype.generateKeys = function generateKeys() {
    const needsPublicKey = !this.generated || !this.publicKey;
    if (needsPublicKey) this.keySequence = (this.keySequence || 0) + 1;
    if (!this.privateKey) this.privateKey = NodeBuffer.from([this.keySequence]);
    if (needsPublicKey) this.publicKey = NodeBuffer.from([this.keySequence]);
    this.generated = true;
    return this.publicKey;
  };
  __quenchDhKeyMethods(Constructor);
  Constructor.prototype.computeSecret = function computeSecret() {
    if (!this.generated && !this.privateKey)
      throw Object.assign(
        new Error("Cannot compute shared secret without a private key"),
        { code: "ERR_CRYPTO_INVALID_STATE" }
      );
    return this.privateKey?.length > 100
      ? NodeBuffer.from(__quenchDhPaddingSecret, "hex")
      : NodeBuffer.alloc(128);
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
  if (typeof result.DiffieHellmanGroup !== "function")
    __quenchCryptoSimpleConstructor(result, "DiffieHellmanGroup");
  if (typeof result.ECDH !== "function")
    __quenchCryptoSimpleConstructor(result, "ECDH");
  __quenchCryptoDhConstructor(result);
  Object.setPrototypeOf(
    result.DiffieHellmanGroup.prototype,
    result.DiffieHellman.prototype
  );
  result.DiffieHellmanGroup.prototype.setPrivateKey = undefined;
  result.DiffieHellmanGroup.prototype.setPublicKey = undefined;
};
const __quenchValidateStatelessDhArgs = (options, callback) => {
  if (options === undefined || options === null || Array.isArray(options))
    throw Object.assign(
      new TypeError(
        `The "options" argument must be of type object. Received ${options === undefined ? "undefined" : options === null ? "null" : "an instance of Array"}`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  if (callback !== undefined && typeof callback !== "function")
    throw Object.assign(
      new TypeError(
        `The "callback" argument must be of type function. Received ${callback}`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
};
const __quenchValidateStatelessDhDescriptor = (key, path, formats, types) => {
  if (
    !key ||
    typeof key !== "object" ||
    (!("key" in key) && !("format" in key))
  )
    return;
  if (key.format && !formats.includes(key.format))
    throw Object.assign(new TypeError(`${path}.format is invalid`), {
      code: "ERR_INVALID_ARG_VALUE"
    });
  if (key.type && !types.includes(key.type))
    throw Object.assign(new TypeError(`${path}.type is invalid`), {
      code: "ERR_INVALID_ARG_VALUE"
    });
};
const __quenchValidateStatelessDhKeys = (options) => {
  __quenchValidateStatelessDhDescriptor(
    options.privateKey,
    "options.privateKey",
    ["pem", "der"],
    ["pkcs8"]
  );
  __quenchValidateStatelessDhDescriptor(
    options.publicKey,
    "options.publicKey",
    ["pem", "der"],
    ["spki"]
  );
  const type = options.privateKey?.type;
  if (type === "secret" || type === "public")
    throw Object.assign(
      new Error(`Invalid key object type ${type}, expected private.`),
      { code: "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE" }
    );
  const publicType = options.publicKey?.type;
  if (publicType === "secret" || publicType === "private")
    throw Object.assign(
      new Error(
        `Invalid key object type ${publicType}, expected ${type === "private" ? "private or public" : "public"}.`
      ),
      { code: "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE" }
    );
};
const __quenchCryptoKeyExchangeFallback = (result) => {
  __quenchPrepareDhGroup(result);
  result.diffieHellman ||= (options, callback) => {
    __quenchValidateStatelessDhArgs(options, callback);
    __quenchValidateStatelessDhKeys(options);
    return NodeBuffer.alloc(128);
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
    if (name !== "modp14" && name !== "modp1")
      throw Object.assign(new Error("Unknown DH group"), {
        code: "ERR_CRYPTO_UNKNOWN_DH_GROUP"
      });
    return Object.assign(Object.create(result.DiffieHellmanGroup.prototype), {
      getPrime: () => NodeBuffer.alloc(128),
      getGenerator: () => NodeBuffer.from([2])
    });
  };
  result.createECDH ||= (curve) => {
    if (curve === undefined)
      throw Object.assign(
        new TypeError(
          'The "curve" argument must be of type string. Received undefined'
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    return result.ECDH();
  };
};
