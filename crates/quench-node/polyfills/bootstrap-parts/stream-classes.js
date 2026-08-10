globalThis.__nodeFs.WriteStream = globalThis.__nodeFs.createWriteStream;
globalThis.__nodeFs.WriteStream.prototype = Object.create(
  NodeWritable.prototype
);
globalThis.__nodeFs.WriteStream.prototype.constructor =
  globalThis.__nodeFs.WriteStream;
// Node's experimental fast UTF-8 writer. Keep the implementation in JS and
// reuse the existing filesystem surface; the host does not need a new stream
// primitive for this API.
globalThis.__nodeFs.Utf8Stream = class Utf8Stream extends NodeEventEmitter {
  constructor(options = {}) {
    super();
    this.path = options.dest;
    this.fd = typeof options.fd === "number" ? options.fd : null;
    this.minLength = options.minLength || 0;
    this.maxLength = options.maxLength || 0;
    this.maxWrite = options.maxWrite || 16384;
    this.periodicFlush = options.periodicFlush || 0;
    this.sync = options.sync === true;
    this.fsync = options.fsync === true;
    this.customFsyncSync = typeof options.fs?.fsyncSync === "function";
    this.append = options.append !== false;
    this.contentMode = options.contentMode || "utf8";
    this.destroyed = false;
    this.closed = false;
    this.ended = false;
    this._buffer = this.contentMode === "buffer" ? [] : "";
    this._bytes = 0;
    this._fs = Object.assign({}, globalThis.__nodeFs, options.fs || {});
    if (this.fd === null && typeof this.path === "string") {
      const flags = this.append ? "a" : "w";
      if (options.mkdir) {
        const parent = this.path.replace(/[\\/][^\\/]*$/, "");
        if (parent) this._fs.mkdirSync(parent, { recursive: true });
      }
      if (this.sync) {
        this.fd = this._fs.openSync(this.path, flags, options.mode);
      } else {
        queueMicrotask(() => {
          try {
            this.fd = this._fs.openSync(this.path, flags, options.mode);
            this.emit("ready");
          } catch (error) {
            this.emit("error", error);
          }
        });
      }
    }
    if (this.fd !== null || this.sync) queueMicrotask(() => this.emit("ready"));
  }
  _asBuffer(value) {
    if (this.contentMode === "buffer") return NodeBuffer.from(value);
    if (typeof value !== "string") {
      throw Object.assign(new TypeError("data must be a string"), { code: "ERR_INVALID_ARG_TYPE" });
    }
    return NodeBuffer.from(value, "utf8");
  }
  _pendingBuffer() {
    return this.contentMode === "buffer"
      ? NodeBuffer.concat(this._buffer)
      : NodeBuffer.from(this._buffer);
  }
  _writeBytes(bytes) {
    if (!bytes.byteLength) return;
    if (this.fd !== null) {
      this._fs.writeSync(this.fd, bytes, 0, bytes.byteLength);
      if (this._fs.fsyncSync && this.fsync) this._fs.fsyncSync(this.fd);
    } else if (this.path) {
      this._fs.writeFileSync(this.path, bytes, {
        flag: this.append ? "a" : "w"
      });
    }
    this.emit("write", bytes.byteLength);
  }
  write(value) {
    if (this.destroyed || this.ended) {
      throw new Error("Utf8Stream is destroyed");
    }
    const bytes = this._asBuffer(value);
    if (this.maxLength && this._bytes + bytes.byteLength > this.maxLength) {
      this.emit("drop", value);
      return false;
    }
    if (this.contentMode === "buffer") this._buffer.push(bytes);
    else this._buffer += String(value);
    this._bytes += bytes.byteLength;
    if (!this.minLength || this._bytes >= this.minLength) {
      if (this.sync) this.flushSync();
      else this.flush();
    }
    return true;
  }
  writeSync(value) {
    const result = this.write(value);
    this.flushSync();
    return result;
  }
  flush(callback) {
    if (typeof callback !== "function") callback = () => {};
    const bytes = this._pendingBuffer();
    this._buffer = this.contentMode === "buffer" ? [] : "";
    this._bytes = 0;
    queueMicrotask(() => {
      if (this.destroyed) {
        callback(null);
        return;
      }
      if (
        this.fd !== null &&
        !this.sync &&
        typeof this._fs.write === "function"
      ) {
        const pieces =
          this.contentMode === "buffer"
            ? Array.from(
                { length: Math.ceil(bytes.length / this.maxWrite) },
                (_, i) =>
                  bytes.subarray(i * this.maxWrite, (i + 1) * this.maxWrite)
              )
            : Array.from(
                { length: Math.ceil(String(bytes).length / this.maxWrite) },
                (_, i) =>
                  String(bytes).slice(
                    i * this.maxWrite,
                    (i + 1) * this.maxWrite
                  )
              );
        if (pieces.length === 0) {
          const finishEmpty = (error) => {
            if (error) {
              this.emit("error", error);
              callback(error);
            } else {
              this.emit("drain");
              callback(null);
            }
          };
          if (this.fsync && this.customFsyncSync) {
            try {
              this._fs.fsyncSync(this.fd);
              finishEmpty(null);
            } catch (error) {
              finishEmpty(error);
            }
          } else finishEmpty(null);
          return;
        }
        let completed = 0;
        let failed = false;
        const done = (index, error, written) => {
          if (failed) return;
          if (error) {
            failed = true;
            this.emit("error", error);
            callback(error);
            return;
          }
          this.emit("write", written ?? NodeBuffer.byteLength(pieces[index]));
          if (++completed === pieces.length) {
            const finish = (error) => {
              if (error) {
                this.emit("error", error);
                callback(error);
              } else {
                this.emit("drain");
                callback(null);
              }
            };
            if (this.fsync && this.customFsyncSync) {
              try {
                this._fs.fsyncSync(this.fd);
                finish(null);
              } catch (error) {
                finish(error);
              }
            } else if (this.fsync && typeof this._fs.fsync === "function") {
              try {
                this._fs.fsync(this.fd, finish);
              } catch (error) {
                finish(error);
              }
            } else finish(null);
          }
        };
        pieces.forEach((piece, index) => {
          try {
            const data = NodeBuffer.from(piece);
            this._fs.write(
              this.fd,
              data,
              0,
              data.byteLength,
              null,
              (error, written) => done(index, error, written)
            );
          } catch (error) {
            done(index, error);
          }
        });
        return;
      }
      try {
        this._writeBytes(bytes);
        this.emit("drain");
        callback(null);
      } catch (error) {
        this.emit("error", error);
        callback(error);
      }
    });
  }
  flushSync() {
    if (this.destroyed) throw new Error("Utf8Stream is destroyed");
    const bytes = this._pendingBuffer();
    this._buffer = this.contentMode === "buffer" ? [] : "";
    this._bytes = 0;
    this._writeBytes(bytes);
    this.emit("drain");
  }
  end(callback) {
    if (typeof callback === "function") this.once("finish", callback);
    if (this.ended) return this;
    this.ended = true;
    try {
      this.sync ? this.flushSync() : this.flush(() => this.emit("finish"));
    } catch (error) {
      this.emit("error", error);
    }
    if (this.sync) {
      queueMicrotask(() => {
        this.emit("finish");
        this.destroy();
      });
    }
    return this;
  }
  reopen(path = this.path, callback) {
    if (this.fd !== null && this._fs.closeSync) this._fs.closeSync(this.fd);
    this.path = path;
    this.fd = this._fs.openSync(path, this.append ? "a" : "w");
    if (callback) queueMicrotask(() => callback(null));
    return this;
  }
  destroy(error) {
    if (this.destroyed) return this;
    this.destroyed = true;
    if (this.fd !== null && this._fs.closeSync) {
      try {
        this._fs.closeSync(this.fd);
      } catch (closeError) {
        error ||= closeError;
      }
    }
    if (error) this.emit("error", error);
    this.closed = true;
    queueMicrotask(() => this.emit("close"));
    return this;
  }
};
