# Node compatibility stages

Each stage is a closed, runnable gate. A stage may be marked complete only
when every JavaScript file in its directory passes; the harness never silently
skips a file. The Node submodule is the upstream compatibility corpus, while
these small fixtures isolate the currently implemented contract.

| Stage | Scope                                                     | Gate                                  |
| ----: | --------------------------------------------------------- | ------------------------------------- |
|     0 | runtime globals and `Buffer`                              | `tools/run-node-tests.sh --stage 0`   |
|     1 | initial CommonJS built-in shims                           | `tools/run-node-tests.sh --stage 1`   |
|     2 | filesystem and path host bindings                         | `tools/run-node-tests.sh --stage 2`   |
|     3 | synchronous file read/write/stat                          | `tools/run-node-tests.sh --stage 3`   |
|     4 | timers and process event listeners                        | `tools/run-node-tests.sh --stage 4`   |
|     5 | `events.EventEmitter`                                     | `tools/run-node-tests.sh --stage 5`   |
|     6 | `os` and `util` modules                                   | `tools/run-node-tests.sh --stage 6`   |
|     7 | `querystring` module                                      | `tools/run-node-tests.sh --stage 7`   |
|     8 | basic `URL` and `URLSearchParams`                         | `tools/run-node-tests.sh --stage 8`   |
|     9 | URL authority and serialization                           | `tools/run-node-tests.sh --stage 9`   |
|    10 | asynchronous `fs` callbacks and promises                  | `tools/run-node-tests.sh --stage 10`  |
|    11 | minimal `stream` readable/writable flow                   | `tools/run-node-tests.sh --stage 11`  |
|    12 | expanded `assert` contract                                | `tools/run-node-tests.sh --stage 12`  |
|    13 | `timers` and `timers/promises` modules                    | `tools/run-node-tests.sh --stage 13`  |
|    14 | process cwd and environment lookup                        | `tools/run-node-tests.sh --stage 14`  |
|    15 | path normalization and decomposition                      | `tools/run-node-tests.sh --stage 15`  |
|    16 | Node `url` module helpers                                 | `tools/run-node-tests.sh --stage 16`  |
|    17 | Rust-backed `crypto.createHash('sha256')`                 | `tools/run-node-tests.sh --stage 17`  |
|    18 | Rust-backed `crypto.randomUUID()`                         | `tools/run-node-tests.sh --stage 18`  |
|    19 | Buffer composition and base64                             | `tools/run-node-tests.sh --stage 19`  |
|    20 | host-backed console output                                | `tools/run-node-tests.sh --stage 20`  |
|    21 | asynchronous assertion helpers                            | `tools/run-node-tests.sh --stage 21`  |
|    22 | promise EventEmitter helpers                              | `tools/run-node-tests.sh --stage 22`  |
|    23 | synchronous directory filesystem APIs                     | `tools/run-node-tests.sh --stage 23`  |
|    24 | Rust-backed filesystem stat kinds                         | `tools/run-node-tests.sh --stage 24`  |
|    25 | synchronous file mutation APIs                            | `tools/run-node-tests.sh --stage 25`  |
|    26 | append and access filesystem APIs                         | `tools/run-node-tests.sh --stage 26`  |
|    27 | filesystem realpath and rm aliases                        | `tools/run-node-tests.sh --stage 27`  |
|    28 | promise directory filesystem APIs                         | `tools/run-node-tests.sh --stage 28`  |
|    29 | POSIX `fs.chmodSync`                                      | `tools/run-node-tests.sh --stage 29`  |
|    30 | POSIX symbolic link APIs                                  | `tools/run-node-tests.sh --stage 30`  |
|    31 | filesystem and OS constants                               | `tools/run-node-tests.sh --stage 31`  |
|    32 | harness no-empty-stage contract                           | `tools/run-node-tests.sh --stage 32`  |
|    33 | upstream `common/tmpdir` helper                           | `tools/run-node-tests.sh --stage 33`  |
|    34 | UTF-8 `TextEncoder` compatibility                         | `tools/run-node-tests.sh --stage 34`  |
|    35 | upstream fs path coercion and async mkdtemp               | `tools/run-node-tests.sh --stage 35`  |
|    36 | Node-compatible six-character mkdtemp suffix              | `tools/run-node-tests.sh --stage 36`  |
|    37 | unique repeated `mkdtemp` prefixes                        | `tools/run-node-tests.sh --stage 37`  |
|    38 | UTF-8 Buffer string round trips                           | `tools/run-node-tests.sh --stage 38`  |
|    39 | host-backed `process.chdir`                               | `tools/run-node-tests.sh --stage 39`  |
|    40 | POSIX `process.umask`                                     | `tools/run-node-tests.sh --stage 40`  |
|    41 | Rust-backed process high-resolution time                  | `tools/run-node-tests.sh --stage 41`  |
|    42 | Rust-backed process identity                              | `tools/run-node-tests.sh --stage 42`  |
|    43 | Rust-derived process platform and arch                    | `tools/run-node-tests.sh --stage 43`  |
|    44 | Node process version metadata                             | `tools/run-node-tests.sh --stage 44`  |
|    45 | process feature/config metadata                           | `tools/run-node-tests.sh --stage 45`  |
|    46 | POSIX process user/group identity                         | `tools/run-node-tests.sh --stage 46`  |
|    47 | Rust-derived OS host values                               | `tools/run-node-tests.sh --stage 47`  |
|    48 | host-derived `os.cpus()`                                  | `tools/run-node-tests.sh --stage 48`  |
|    49 | harness `.js` and `.mjs` coverage                         | `tools/run-node-tests.sh --stage 49`  |
|    50 | console timing APIs                                       | `tools/run-node-tests.sh --stage 50`  |
|    51 | console counters and clear                                | `tools/run-node-tests.sh --stage 51`  |
|    52 | `assert.strict` namespace                                 | `tools/run-node-tests.sh --stage 52`  |
|    53 | upstream `common.mustCall` helpers                        | `tools/run-node-tests.sh --stage 53`  |
|    54 | exact callback-count enforcement                          | `tools/run-node-tests.sh --stage 54`  |
|    55 | process warning emission                                  | `tools/run-node-tests.sh --stage 55`  |
|    56 | process executable identity                               | `tools/run-node-tests.sh --stage 56`  |
|    57 | host command-line `process.argv`                          | `tools/run-node-tests.sh --stage 57`  |
|    58 | mutable host-backed `process.env`                         | `tools/run-node-tests.sh --stage 58`  |
|    59 | process environment enumeration                           | `tools/run-node-tests.sh --stage 59`  |
|    60 | `buffer` atob/btoa exports                                | `tools/run-node-tests.sh --stage 60`  |
|    61 | diagnostic mkdtemp input matrix                           | `tools/run-node-tests.sh --stage 61`  |
|    62 | diagnostic async mkdtemp callbacks                        | `tools/run-node-tests.sh --stage 62`  |
|    63 | JavaScript exception diagnostics                          | `tools/run-node-tests.sh --stage 63`  |
|    64 | Node tmpdir template basename semantics                   | `tools/run-node-tests.sh --stage 64`  |
|    65 | strict callback receiver semantics                        | `tools/run-node-tests.sh --stage 65`  |
|    66 | UTF-8 Uint8Array filesystem paths                         | `tools/run-node-tests.sh --stage 66`  |
|    67 | upstream common platform helpers                          | `tools/run-node-tests.sh --stage 67`  |
|    68 | callback and recursive `fs.mkdir` compatibility           | `tools/run-node-tests.sh --stage 68`  |
|    69 | basic filesystem open/close and directory enumeration     | `tools/run-node-tests.sh --stage 69`  |
|    70 | callback and promise filesystem open handles              | `tools/run-node-tests.sh --stage 70`  |
|    71 | common success callbacks and isolated tmpdir paths        | `tools/run-node-tests.sh --stage 71`  |
|    72 | `__filename` initialization and async `fs.readdir` errors | `tools/run-node-tests.sh --stage 72`  |
|    73 | synchronous filesystem open validation and `ENOENT`       | `tools/run-node-tests.sh --stage 73`  |
|    74 | filesystem stat/lstat/fstat metadata contracts            | `tools/run-node-tests.sh --stage 74`  |
|    75 | stat options and missing-entry behavior                   | `tools/run-node-tests.sh --stage 75`  |
|    76 | `Buffer.allocUnsafe` compatibility                        | `tools/run-node-tests.sh --stage 76`  |
|    77 | binary Buffer-backed filesystem read/write                | `tools/run-node-tests.sh --stage 77`  |
|    78 | AbortSignal-aware `fs.readFile` behavior                  | `tools/run-node-tests.sh --stage 78`  |
|    79 | empty-file and encoding-aware `fs.readFile` behavior      | `tools/run-node-tests.sh --stage 79`  |
|    80 | common platform skip flags                                | `tools/run-node-tests.sh --stage 80`  |
|    81 | callback and Buffer-aware `fs.writeFile`                  | `tools/run-node-tests.sh --stage 81`  |
|    82 | foundational promise-based filesystem write/read          | `tools/run-node-tests.sh --stage 82`  |
|    83 | minimal `internal/test/binding` facade                    | `tools/run-node-tests.sh --stage 83`  |
|    84 | `Buffer.equals` comparison                                | `tools/run-node-tests.sh --stage 84`  |
|    85 | realpath and symlink capability helpers                   | `tools/run-node-tests.sh --stage 85`  |
|    86 | copy-file flags and UV constants                          | `tools/run-node-tests.sh --stage 86`  |
|    87 | copy-file path, mode, and callback validation             | `tools/run-node-tests.sh --stage 87`  |
|    88 | async rename/rmdir and duplicate mkdir errors             | `tools/run-node-tests.sh --stage 88`  |
|    89 | common invalid-argument diagnostic helper                 | `tools/run-node-tests.sh --stage 89`  |
|    90 | callback and Buffer-aware `fs.appendFile`                 | `tools/run-node-tests.sh --stage 90`  |
|    91 | asynchronous chmod and permission metadata                | `tools/run-node-tests.sh --stage 91`  |
|    92 | fd-based `fchmod` compatibility                           | `tools/run-node-tests.sh --stage 92`  |
|    93 | sync, async, and promise `fs.access`                      | `tools/run-node-tests.sh --stage 93`  |
|    94 | `appendFileSync` data validation                          | `tools/run-node-tests.sh --stage 94`  |
|    95 | foundational promise-based `appendFile`                   | `tools/run-node-tests.sh --stage 95`  |
|    96 | sync/async `fs.statfs` metadata                           | `tools/run-node-tests.sh --stage 96`  |
|    97 | sync/async/promise filesystem truncate                    | `tools/run-node-tests.sh --stage 97`  |
|    98 | callback and promise `fs.rm`                              | `tools/run-node-tests.sh --stage 98`  |
|    99 | file/directory-aware `fs.rmSync` behavior                 | `tools/run-node-tests.sh --stage 99`  |
|   100 | `fs.readdir` `withFileTypes` Dirent support               | `tools/run-node-tests.sh --stage 100` |
|   101 | internal filesystem open-flag utility                     | `tools/run-node-tests.sh --stage 101` |
|   102 | numeric and octal-string `fs.open` modes                  | `tools/run-node-tests.sh --stage 102` |
|   103 | `fs.close` fd and callback validation                     | `tools/run-node-tests.sh --stage 103` |
|   104 | fd mode-mask regression coverage                          | `tools/run-node-tests.sh --stage 104` |
|   105 | promise-based readdir Dirents                             | `tools/run-node-tests.sh --stage 105` |
|   106 | sync/async/promise directory handles                      | `tools/run-node-tests.sh --stage 106` |
|   107 | async symlink and readlink callbacks                      | `tools/run-node-tests.sh --stage 107` |
|   108 | symlink-aware `lstat` metadata                            | `tools/run-node-tests.sh --stage 108` |
|   109 | promise-based symlink and readlink                        | `tools/run-node-tests.sh --stage 109` |
|   110 | sync, callback, and promise `fs.unlink`                   | `tools/run-node-tests.sh --stage 110` |
|   111 | sync, callback, and promise `fs.link`                     | `tools/run-node-tests.sh --stage 111` |
|   112 | options-object `fs.read` sync/callback forms              | `tools/run-node-tests.sh --stage 112` |
|   113 | Buffer and null-offset `fs.read` options                  | `tools/run-node-tests.sh --stage 113` |
|   114 | `fs.read` offset and position validation                  | `tools/run-node-tests.sh --stage 114` |
|   115 | sync, callback, and promise vectored `fs.readv`           | `tools/run-node-tests.sh --stage 115` |
|   116 | sync write/read and vectored `fs.writev`                  | `tools/run-node-tests.sh --stage 116` |
|   117 | `fs.open` write-mode truncation                           | `tools/run-node-tests.sh --stage 117` |
|   118 | Buffer equality after vectored file writes                | `tools/run-node-tests.sh --stage 118` |
|   119 | `fs.writev` invalid-buffer validation                     | `tools/run-node-tests.sh --stage 119` |
|   120 | promise-based vectored `fs.readv`                         | `tools/run-node-tests.sh --stage 120` |
|   121 | promise-based vectored `fs.writev`                        | `tools/run-node-tests.sh --stage 121` |
|   122 | options-object callback `fs.write`                        | `tools/run-node-tests.sh --stage 122` |
|   123 | `util.promisify` callback conversion                      | `tools/run-node-tests.sh --stage 123` |
|   124 | promise file-handle buffer reads                          | `tools/run-node-tests.sh --stage 124` |
|   125 | fd-based `fs.writeFileSync`                               | `tools/run-node-tests.sh --stage 125` |
|   126 | typed-array `fs.writeFileSync` input                      | `tools/run-node-tests.sh --stage 126` |
|   127 | DataView `fs.writeFileSync` input                         | `tools/run-node-tests.sh --stage 127` |
|   128 | upstream common ArrayBuffer view helper                   | `tools/run-node-tests.sh --stage 128` |
|   129 | `fs.writeFileSync` encoding and append options            | `tools/run-node-tests.sh --stage 129` |
|   130 | `fs.writeFileSync` flush option                           | `tools/run-node-tests.sh --stage 130` |
|   131 | fd-based callback `fs.appendFile`                         | `tools/run-node-tests.sh --stage 131` |
|   132 | fd-based promise `fs.appendFile`                          | `tools/run-node-tests.sh --stage 132` |
|   133 | sync `fs.ftruncate` size changes                          | `tools/run-node-tests.sh --stage 133` |
|   134 | invalid callback `fs.truncate` lengths                    | `tools/run-node-tests.sh --stage 134` |
|   135 | fractional `fs.truncateSync` lengths                      | `tools/run-node-tests.sh --stage 135` |
|   136 | encoded synchronous `fs.readFileSync` results             | `tools/run-node-tests.sh --stage 136` |
|   137 | buffer option on synchronous `fs.readFileSync`            | `tools/run-node-tests.sh --stage 137` |
|   138 | create behavior of `fs.readFileSync` append mode          | `tools/run-node-tests.sh --stage 138` |
|   139 | default permission mode for `fs.writeFileSync`            | `tools/run-node-tests.sh --stage 139` |
