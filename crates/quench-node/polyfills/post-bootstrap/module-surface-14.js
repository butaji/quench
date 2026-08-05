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
  Constructor.prototype.getPublicKey = function getPublicKey() {
    if (!this.generated && !this.privateKey) __quenchCryptoInvalidState();
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
  Constructor.prototype.computeSecret = function computeSecret() {
    if (!this.generated)
      throw Object.assign(
        new Error("Cannot compute shared secret without a private key"),
        { code: "ERR_CRYPTO_INVALID_STATE" }
      );
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
const __quenchCryptoKeyExchangeFallback = (result) => {
  __quenchPrepareDhGroup(result);
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
