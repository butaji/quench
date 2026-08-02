const { Buffer } = require("buffer");

const buffer = Buffer.from("abracadabra");
if (buffer.indexOf("abra") !== 0) throw new Error("indexOf failed");
if (buffer.indexOf("abra", 1) !== 7) throw new Error("offset indexOf failed");
if (buffer.lastIndexOf("a") !== 10) throw new Error("lastIndexOf failed");
if (buffer.indexOf(0x7a) !== -1) throw new Error("missing search failed");
