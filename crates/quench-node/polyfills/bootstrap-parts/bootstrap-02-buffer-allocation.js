const __nodeBufferAllocate = (size, fill, encoding) => {
  const length = NodeBuffer._validateSize(size);
  __nodeAllocatorCounts.zeroFilled++;
  return new NodeBuffer(length).fill(fill, 0, length, encoding);
};
const __NodeBufferBase04 = NodeBuffer;
NodeBuffer = class NodeBuffer extends __NodeBufferBase04 {
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
