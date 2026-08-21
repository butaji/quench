const assert = require("assert");
const { Duplex } = require("stream");

const duplex = Duplex({ readable: false });
duplex.on("end", () => assert.fail("disabled readable emitted end"));
duplex.resume();
setTimeout(() => console.log("stream duplex disabled readable end pass"), 0);
