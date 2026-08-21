function REPLServer(input, output, options) {
  this.input = input;
  this.output = output;
  this.options = options || {};
  this.context = this.options.context || {};
  this._prompt = this.options.prompt || '> ';
  this.closed = false;
}
REPLServer.prototype.setPrompt = function (prompt) { this._prompt = String(prompt); };
REPLServer.prototype.prompt = function () {
  if (this.closed) return;
  if (this.output && typeof this.output.write === 'function') this.output.write(this._prompt);
};
REPLServer.prototype.close = function () { this.closed = true; return this; };
REPLServer.prototype.on = function () { return this; };
module.exports = {
  start: function (options) {
    options = options || {};
    return new REPLServer(options.input, options.output, options);
  },
  REPLServer: REPLServer
};