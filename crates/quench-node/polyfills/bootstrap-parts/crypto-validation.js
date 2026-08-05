const __nodeCryptoRandomArguments = (minimum, maximum, callback) => {
  if (typeof minimum === "function")
    return { minimum: 0, maximum: 0x1_0000_0000_0000, callback: minimum };
  if (typeof maximum === "function")
    return { minimum: 0, maximum: minimum, callback: maximum };
  if (maximum === undefined) return { minimum: 0, maximum: minimum, callback };
  return { minimum, maximum, callback };
};
const __nodeCryptoCipherInfo = (name) =>
  String(name).toLowerCase() === "aes-128-cbc"
    ? {
        name: "aes-128-cbc",
        nid: 419,
        blockSize: 16,
        ivLength: 16,
        keyLength: 16,
        mode: "cbc"
      }
    : undefined;
