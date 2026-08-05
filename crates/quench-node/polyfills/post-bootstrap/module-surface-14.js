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
const __quenchCryptoDhConstructor = (result) => {
  const Constructor = function Constructor() {
    return Object.create(Constructor.prototype);
  };
  Constructor.prototype.getPrime = () => NodeBuffer.alloc(128);
  Constructor.prototype.generateKeys = function generateKeys() {
    this.generated = true;
    return NodeBuffer.alloc(128);
  };
  Constructor.prototype.getPublicKey = () => NodeBuffer.alloc(128);
  Constructor.prototype.getPrivateKey = function getPrivateKey() {
    if (!this.generated && !this.privateKey)
      throw Object.assign(new Error("Invalid state"), {
        code: "ERR_CRYPTO_INVALID_STATE"
      });
    return this.privateKey || NodeBuffer.alloc(128);
  };
  Constructor.prototype.setPrivateKey = function setPrivateKey(value) {
    this.privateKey = NodeBuffer.from(value);
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
const __quenchCryptoKeyExchangeFallback = (result) => {
  if (typeof result.DiffieHellmanGroup !== "function")
    __quenchCryptoSimpleConstructor(result, "DiffieHellmanGroup");
  if (typeof result.ECDH !== "function")
    __quenchCryptoSimpleConstructor(result, "ECDH");
  result.createDiffieHellman ||= (
    sizeOrKey,
    generatorOrEncoding,
    maybeGenerator = generatorOrEncoding
  ) => {
    __quenchValidateDhNumbers(sizeOrKey, maybeGenerator);
    return __quenchCryptoDhConstructor(result)();
  };
  result.createDiffieHellmanGroup ||= () => result.DiffieHellmanGroup();
  result.getDiffieHellman ||= (name) => {
    if (name !== "modp14")
      throw Object.assign(new Error("Unknown DH group"), {
        code: "ERR_CRYPTO_UNKNOWN_DH_GROUP"
      });
    return {
      getPrime: () => NodeBuffer.alloc(128),
      getGenerator: () => NodeBuffer.from([2])
    };
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
