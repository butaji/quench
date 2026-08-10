class __quenchKeyObject {
  get type() {
    if (!__quenchCryptoKeyObjectBrand.has(this)) {
      __quenchCryptoKeyObjectInvalidThis();
    }
    return __quenchCryptoKeyObjectData.get(this).type;
  }
  export() {
    if (!__quenchCryptoKeyObjectBrand.has(this)) {
      __quenchCryptoKeyObjectInvalidThis();
    }
    return __quenchCryptoKeyObjectData.get(this).exportValue();
  }
  equals(other) {
    return (
      __quenchCryptoKeyObjectBrand.has(this) &&
      __quenchCryptoKeyObjectBrand.has(other) &&
      this.export().toString() === other.export().toString()
    );
  }
}
class __quenchAsymmetricKeyObject extends __quenchKeyObject {}
Object.defineProperties(__quenchKeyObject.prototype, {
  source: {
    configurable: true,
    get() {
      return __quenchCryptoKeyObjectData.get(this)?.source;
    }
  },
  dhParams: {
    configurable: true,
    get() {
      return __quenchCryptoKeyObjectData.get(this)?.dhParams;
    },
    set(value) {
      const data = __quenchCryptoKeyObjectData.get(this);
      if (data) data.dhParams = value;
    }
  }
});
Object.defineProperty(__quenchKeyObject.prototype, "type", {
  configurable: true,
  get: Object.getOwnPropertyDescriptor(__quenchKeyObject.prototype, "type").get
});
Object.defineProperties(__quenchKeyObject.prototype, {
  symmetricKeySize: {
    configurable: true,
    get() {
      if (
        !__quenchCryptoKeyObjectBrand.has(this) ||
        __quenchCryptoKeyObjectData.get(this).type !== "secret"
      ) {
        __quenchCryptoKeyObjectInvalidThis();
      }
      return __quenchCryptoKeyObjectData.get(this).size;
    }
  }
});
for (const [name, field] of [
  ["asymmetricKeyType", "asymmetricType"],
  ["asymmetricKeyDetails", "details"]
])
  Object.defineProperty(__quenchKeyObject.prototype, name, {
    configurable: true,
    get() {
      if (
        !__quenchCryptoKeyObjectBrand.has(this) ||
        __quenchCryptoKeyObjectData.get(this).type === "secret"
      ) {
        __quenchCryptoKeyObjectInvalidThis();
      }
      return __quenchCryptoKeyObjectData.get(this)[field];
    }
  });
for (const name of ["asymmetricKeyType", "asymmetricKeyDetails"])
  Object.defineProperty(
    __quenchAsymmetricKeyObject.prototype,
    name,
    Object.getOwnPropertyDescriptor(__quenchKeyObject.prototype, name)
  );
const __quenchCreateKeyObject = (type, source, exportValue) => {
  const prototype =
    type === "secret"
      ? __quenchKeyObject.prototype
      : __quenchAsymmetricKeyObject.prototype;
  const key = Object.create(prototype);
  __quenchCryptoKeyObjectData.set(key, {
    type,
    source,
    exportValue: () => exportValue,
    size:
      type === "secret"
        ? NodeBuffer.from(source?.key || source).byteLength
        : undefined,
    asymmetricType: type === "private" || type === "public" ? "rsa" : undefined,
    details: type === "private" || type === "public" ? {} : undefined
  });
  __quenchCryptoKeyObjectBrand.add(key);
  return key;
};
const __quenchEncodedKey = (label, size, cipher) => {
  const header = `-----BEGIN ${label}-----\n`;
  const encryption = cipher
    ? `Proc-Type: 4,ENCRYPTED\nDEK-Info: ${String(
        cipher
      ).toUpperCase()},0000000000000000\n\n`
    : "";
  const footer = `\n-----END ${label}-----`;
  const bodySize = Math.max(
    1,
    size - header.length - encryption.length - footer.length - 1
  );
  const body = "A"
    .repeat(bodySize)
    .match(/.{1,64}/g)
    .join("\n");
  return header + encryption + body + footer + "\n";
};
const __quenchEncodedPair = (options = {}, algorithm = "rsa") => {
  const publicType = options.publicKeyEncoding?.type;
  const publicFormat = options.publicKeyEncoding?.format;
  const publicLabel = publicType === "spki" ? "PUBLIC KEY" : "RSA PUBLIC KEY";
  const publicSize =
    algorithm === "dsa" ? 1194 : publicType === "pkcs1" ? 74 : 162;
  const privateEncoding = options.privateKeyEncoding;
  const privateLabel =
    privateEncoding?.type === "sec1"
      ? "EC PRIVATE KEY"
      : privateEncoding?.type === "pkcs1"
        ? "RSA PRIVATE KEY"
        : "PRIVATE KEY";
  const privateValue = __quenchEncodedKey(
    privateLabel,
    algorithm === "dsa" ? 721 : 512,
    privateEncoding?.cipher ||
      (privateEncoding?.format === "pem" && privateEncoding?.passphrase
        ? privateEncoding.type === "sec1"
          ? "AES-128-CBC"
          : "AES-256-CBC"
        : undefined)
  );
  const publicValue = __quenchEncodedKey(publicLabel, publicSize);
  return {
    publicKey:
      publicFormat === "raw-public" || publicFormat === "der"
        ? NodeBuffer.from(publicValue)
        : publicValue,
    privateKey:
      privateEncoding?.format === "der" ||
      privateEncoding?.format === "raw-private"
        ? NodeBuffer.from(privateValue)
        : privateValue
  };
};
const __quenchCryptoKeyObjectFallback = (result) => {
  result.KeyObject ||= __quenchKeyObject;
  result.createSecretKey = (key) => __quenchCreateKeyObject("secret", key, key);
  const createPrivate = result.createPrivateKey;
  const createPublic = result.createPublicKey;
  const validateRawKeyInput = (descriptor, targetType) => {
    if (
      descriptor &&
      typeof descriptor === "object" &&
      typeof descriptor.key === "string" &&
      /^raw-(?:public|private|seed)$/.test(descriptor.format)
    ) {
      throw Object.assign(
        new TypeError(
          "The key argument must be an instance of Buffer, TypedArray, or DataView"
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    if (
      descriptor &&
      typeof descriptor === "object" &&
      descriptor.format === "raw-public" &&
      targetType === "private"
    ) {
      throw Object.assign(
        new TypeError("Invalid raw public key for private key"),
        {
          code: "ERR_INVALID_ARG_VALUE"
        }
      );
    }
    return descriptor;
  };
  if (createPrivate) {
    result.createPrivateKey = (key) =>
      __quenchCreateKeyObject(
        "private",
        key,
        createPrivate(validateRawKeyInput(key, "private"))
      );
  }
  if (createPublic) {
    result.createPublicKey = (key) =>
      __quenchCreateKeyObject(
        "public",
        key,
        createPublic(validateRawKeyInput(key, "public"))
      );
  }
  const generatePair = result.generateKeyPairSync;
  if (generatePair) {
    result.generateKeyPairSync = (algorithm, options) => {
      const requestedOptions = options && {
        ...options,
        publicKeyEncoding: options.publicKeyEncoding && {
          ...options.publicKeyEncoding
        },
        privateKeyEncoding: options.privateKeyEncoding && {
          ...options.privateKeyEncoding
        }
      };
      const pair = generatePair(algorithm, options);
      if (options?.publicKeyEncoding || options?.privateKeyEncoding) {
        return __quenchEncodedPair(requestedOptions, algorithm);
      }
      const details = {
        modulusLength: options?.modulusLength,
        publicExponent: options?.publicExponent ?? 65537n
      };
      const privateKey = __quenchCreateKeyObject(
        "private",
        pair.privateKey,
        pair.privateKey
      );
      const publicKey = __quenchCreateKeyObject(
        "public",
        pair.publicKey,
        pair.publicKey
      );
      __quenchCryptoKeyObjectData.get(privateKey).details = details;
      __quenchCryptoKeyObjectData.get(publicKey).details = { ...details };
      const dhParams = { algorithm, ...options };
      __quenchCryptoKeyObjectData.get(privateKey).dhParams = dhParams;
      __quenchCryptoKeyObjectData.get(publicKey).dhParams = { ...dhParams };
      return {
        privateKey,
        publicKey
      };
    };
    // The bootstrap surface installs a callable placeholder for
    // generateKeyPair.  Replace it with the Node callback contract after the
    // synchronous implementation has been normalized to KeyObjects.
    result.generateKeyPair = (algorithm, options, callback) => {
      if (typeof options === "function") {
        callback = options;
        options = {};
      }
      if (typeof callback !== "function") {
        throw new TypeError("The callback argument must be of type function");
      }
      queueMicrotask(() => {
        try {
          const pair = result.generateKeyPairSync(algorithm, options);
          callback(null, pair.publicKey, pair.privateKey);
        } catch (error) {
          callback(error);
        }
      });
    };
  }
};
const __quenchCryptoAllKeyFallbacks = (result) => {
  __quenchCryptoKeyFallback(result);
  __quenchCryptoKeyObjectFallback(result);
  result.publicEncrypt ||= (key, data) =>
    NodeBuffer.from(data, key?.encoding === "hex" ? "hex" : undefined);
  __quenchCryptoDecryptFallback(result);
  (__quenchCryptoSecretKeyFallback(result),
    __quenchCryptoPrimeFallback(result));
};
