// Self-hosted DataView prototype methods
// DataView.getInt8, .getUint8, .setInt8, .setUint8, .getInt16, .getUint16,
// .setInt16, .setUint16, .getInt32, .getUint32, .setInt32, .setUint32,
// .getFloat32, .getFloat64, .setFloat32, .setFloat64, .getBigInt64,
// .getBigUint64, .setBigInt64, .setBigUint64 — these need native Rust
// implementations first before they can be wrapped here.
//
// Once the native methods exist on DataView.prototype in Rust, add:
//   var _nativeGetInt8 = DataView.prototype.getInt8;
//   var _nativeSetInt8 = DataView.prototype.setInt8;
//   ... etc.
//
// Then wrap each:
//   DataView.prototype.getInt8 = function(byteOffset) {
//     if (this === null || this === undefined) throw ThrowTypeError("DataView.prototype.getInt8 called on null or undefined");
//     return _nativeGetInt8.apply(this, arguments);
//   };
//
// Note: DataView constructor is registered in Rust at
// crates/quench-runtime/src/builtins/data_view.rs.
