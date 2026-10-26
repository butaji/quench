# TOD-20 VM global setter probe

The frozen census failure in `test-vm-global-setter.js` showed a Node `vm` global assignment raising `Cannot redefine property: nonWritableProp`, while Quench raised `Cannot assign to read only property ...`. A narrow guard was tried in `vm_shared_vm.rs`, first after the `receiver !== global` fast path and then before it. Neither attempt changed the official fixture.

The first focused run over the 12 `test-vm-global*` fixtures reported 9 pass / 3 fail / 0 skip, timeout, crash, or unclassified. Results SHA-256 `4f26dad8cf74b996fb7a127faf6248f38b11063308dfeec663f47670bd479511`; checkpoint SHA-256 `91f9f58fcb90ccebfe01061396940421e0e9deebfa151a09758585b86a74cbf4`. The 3 failures were `test-vm-global-property-interceptors.js`, `test-vm-global-property-prototype.js`, and `test-vm-global-setter.js`.

The second run isolated `test-vm-global-setter.js`; it still failed with the same message mismatch. Results SHA-256 `7e40d33f3a04253cd8ca41b156a21f82588d186322bc0ea744701fb5ef4f0939`; checkpoint SHA-256 `56cc1ac57ae45b6040a5d5e93315bc5c3c187099006863e62ba808471068119b`.

The reason is subsystem ownership: this official fixture enters Node's native `contextify` binding path. The edited `vm_shared_vm.rs` proxy is not on this execution path, so the guard could never affect the observed result. Both edits have been reverted. No source behavior changed, no results count toward the census, and the next VM attempt must target the native Contextify/V8 global path rather than the public JS vm wrapper. The final experimental runner binary SHA-256 is `9db7e7fe9405a110a331d41be0b70ba1d3ce8af2cf1b4e3df2ee60a4901ebef7` and does not match the restored source; rebuild before further validation.
