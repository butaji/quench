if (globalThis.Blob?.prototype) {
  globalThis.Blob.prototype.stream = function () {
    const blob = this;
    return new __quenchReadableStream({
      start: (controller) => {
        if (blob._data) {
          controller.enqueue(blob._data);
          controller.close();
        } else {
          Promise.resolve(blob.arrayBuffer()).then((buffer) => {
            controller.enqueue(new Uint8Array(buffer));
            controller.close();
          });
        }
      },
    });
  };
}
