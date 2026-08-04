const __nodeBufferAllocate = (size, fill, encoding) => {
  const length = NodeBuffer._validateSize(size);
  __nodeAllocatorCounts.zeroFilled++;
  return new NodeBuffer(length).fill(fill, 0, length, encoding);
};
const __NodeBufferBase04 = NodeBuffer;
NodeBuffer = class NodeBuffer extends __NodeBufferBase04 {
  subarray(begin = 0, end = this.length) {
    const view = Uint8Array.prototype.subarray.call(this, begin, end);
    return new NodeBuffer(view.buffer, view.byteOffset, view.byteLength);
  }
  slice(begin = 0, end = this.length) {
    return this.subarray(begin, end);
  }
  static copyBytesFrom(view, offset = 0, length) {
    const result = __NodeBufferBase04.copyBytesFrom(view, offset, length);
    return NodeBuffer.from(result);
  }
  static of(...values) {
    return new NodeBuffer(values);
  }
  static alloc(size, fill = 0, encoding) {
    return __nodeBufferAllocate(size, fill, encoding);
  }
  static allocUnsafe(size) {
    return new NodeBuffer(NodeBuffer._validateSize(size));
  }
  static allocUnsafeSlow(size) {
    return new NodeBuffer(NodeBuffer._validateSize(size));
  }
};
