if (Object.prototype.toString.call(process) !== "[object process]") {
  throw new Error("process object has the wrong brand");
}
