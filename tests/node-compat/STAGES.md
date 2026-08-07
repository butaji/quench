# Node compatibility stages

Each stage is a closed, runnable gate. A stage may be marked complete only when
every JavaScript file in its directory passes; the harness never silently skips
a file. The Node submodule is the upstream compatibility corpus, while these
small fixtures isolate the currently implemented contract.

| Stage | Scope                                                                    | Gate                                  |
| ----: | ------------------------------------------------------------------------ | ------------------------------------- |
|     0 | runtime globals and `Buffer`                                             | `tools/run-node-tests.sh --stage 0`   |
|     1 | initial CommonJS built-in shims                                          | `tools/run-node-tests.sh --stage 1`   |
|     2 | filesystem and path host bindings                                        | `tools/run-node-tests.sh --stage 2`   |
|     3 | synchronous file read/write/stat                                         | `tools/run-node-tests.sh --stage 3`   |
|     4 | timers and process event listeners                                       | `tools/run-node-tests.sh --stage 4`   |
|     5 | `events.EventEmitter`                                                    | `tools/run-node-tests.sh --stage 5`   |
|     6 | `os` and `util` modules                                                  | `tools/run-node-tests.sh --stage 6`   |
|     7 | `querystring` module                                                     | `tools/run-node-tests.sh --stage 7`   |
|     8 | basic `URL` and `URLSearchParams`                                        | `tools/run-node-tests.sh --stage 8`   |
|     9 | URL authority and serialization                                          | `tools/run-node-tests.sh --stage 9`   |
|    10 | asynchronous `fs` callbacks and promises                                 | `tools/run-node-tests.sh --stage 10`  |
|    11 | minimal `stream` readable/writable flow                                  | `tools/run-node-tests.sh --stage 11`  |
|    12 | expanded `assert` contract                                               | `tools/run-node-tests.sh --stage 12`  |
|    13 | `timers` and `timers/promises` modules                                   | `tools/run-node-tests.sh --stage 13`  |
|    14 | process cwd and environment lookup                                       | `tools/run-node-tests.sh --stage 14`  |
|    15 | path normalization and decomposition                                     | `tools/run-node-tests.sh --stage 15`  |
|    16 | Node `url` module helpers                                                | `tools/run-node-tests.sh --stage 16`  |
|    17 | Rust-backed `crypto.createHash('sha256')`                                | `tools/run-node-tests.sh --stage 17`  |
|    18 | Rust-backed `crypto.randomUUID()`                                        | `tools/run-node-tests.sh --stage 18`  |
|    19 | Buffer composition and base64                                            | `tools/run-node-tests.sh --stage 19`  |
|    20 | host-backed console output                                               | `tools/run-node-tests.sh --stage 20`  |
|    21 | asynchronous assertion helpers                                           | `tools/run-node-tests.sh --stage 21`  |
|    22 | promise EventEmitter helpers                                             | `tools/run-node-tests.sh --stage 22`  |
|    23 | synchronous directory filesystem APIs                                    | `tools/run-node-tests.sh --stage 23`  |
|    24 | Rust-backed filesystem stat kinds                                        | `tools/run-node-tests.sh --stage 24`  |
|    25 | synchronous file mutation APIs                                           | `tools/run-node-tests.sh --stage 25`  |
|    26 | append and access filesystem APIs                                        | `tools/run-node-tests.sh --stage 26`  |
|    27 | filesystem realpath and rm aliases                                       | `tools/run-node-tests.sh --stage 27`  |
|    28 | promise directory filesystem APIs                                        | `tools/run-node-tests.sh --stage 28`  |
|    29 | POSIX `fs.chmodSync`                                                     | `tools/run-node-tests.sh --stage 29`  |
|    30 | POSIX symbolic link APIs                                                 | `tools/run-node-tests.sh --stage 30`  |
|    31 | filesystem and OS constants                                              | `tools/run-node-tests.sh --stage 31`  |
|    32 | harness no-empty-stage contract                                          | `tools/run-node-tests.sh --stage 32`  |
|    33 | upstream `common/tmpdir` helper                                          | `tools/run-node-tests.sh --stage 33`  |
|    34 | UTF-8 `TextEncoder` compatibility                                        | `tools/run-node-tests.sh --stage 34`  |
|    35 | upstream fs path coercion and async mkdtemp                              | `tools/run-node-tests.sh --stage 35`  |
|    36 | Node-compatible six-character mkdtemp suffix                             | `tools/run-node-tests.sh --stage 36`  |
|    37 | unique repeated `mkdtemp` prefixes                                       | `tools/run-node-tests.sh --stage 37`  |
|    38 | UTF-8 Buffer string round trips                                          | `tools/run-node-tests.sh --stage 38`  |
|    39 | host-backed `process.chdir`                                              | `tools/run-node-tests.sh --stage 39`  |
|    40 | POSIX `process.umask`                                                    | `tools/run-node-tests.sh --stage 40`  |
|    41 | Rust-backed process high-resolution time                                 | `tools/run-node-tests.sh --stage 41`  |
|    42 | Rust-backed process identity                                             | `tools/run-node-tests.sh --stage 42`  |
|    43 | Rust-derived process platform and arch                                   | `tools/run-node-tests.sh --stage 43`  |
|    44 | Node process version metadata                                            | `tools/run-node-tests.sh --stage 44`  |
|    45 | process feature/config metadata                                          | `tools/run-node-tests.sh --stage 45`  |
|    46 | POSIX process user/group identity                                        | `tools/run-node-tests.sh --stage 46`  |
|    47 | Rust-derived OS host values                                              | `tools/run-node-tests.sh --stage 47`  |
|    48 | host-derived `os.cpus()`                                                 | `tools/run-node-tests.sh --stage 48`  |
|    49 | harness `.js` and `.mjs` coverage                                        | `tools/run-node-tests.sh --stage 49`  |
|    50 | console timing APIs                                                      | `tools/run-node-tests.sh --stage 50`  |
|    51 | console counters and clear                                               | `tools/run-node-tests.sh --stage 51`  |
|    52 | `assert.strict` namespace                                                | `tools/run-node-tests.sh --stage 52`  |
|    53 | upstream `common.mustCall` helpers                                       | `tools/run-node-tests.sh --stage 53`  |
|    54 | exact callback-count enforcement                                         | `tools/run-node-tests.sh --stage 54`  |
|    55 | process warning emission                                                 | `tools/run-node-tests.sh --stage 55`  |
|    56 | process executable identity                                              | `tools/run-node-tests.sh --stage 56`  |
|    57 | host command-line `process.argv`                                         | `tools/run-node-tests.sh --stage 57`  |
|    58 | mutable host-backed `process.env`                                        | `tools/run-node-tests.sh --stage 58`  |
|    59 | process environment enumeration                                          | `tools/run-node-tests.sh --stage 59`  |
|    60 | `buffer` atob/btoa exports                                               | `tools/run-node-tests.sh --stage 60`  |
|    61 | diagnostic mkdtemp input matrix                                          | `tools/run-node-tests.sh --stage 61`  |
|    62 | diagnostic async mkdtemp callbacks                                       | `tools/run-node-tests.sh --stage 62`  |
|    63 | JavaScript exception diagnostics                                         | `tools/run-node-tests.sh --stage 63`  |
|    64 | Node tmpdir template basename semantics                                  | `tools/run-node-tests.sh --stage 64`  |
|    65 | strict callback receiver semantics                                       | `tools/run-node-tests.sh --stage 65`  |
|    66 | UTF-8 Uint8Array filesystem paths                                        | `tools/run-node-tests.sh --stage 66`  |
|    67 | upstream common platform helpers                                         | `tools/run-node-tests.sh --stage 67`  |
|    68 | callback and recursive `fs.mkdir` compatibility                          | `tools/run-node-tests.sh --stage 68`  |
|    69 | basic filesystem open/close and directory enumeration                    | `tools/run-node-tests.sh --stage 69`  |
|    70 | callback and promise filesystem open handles                             | `tools/run-node-tests.sh --stage 70`  |
|    71 | common success callbacks and isolated tmpdir paths                       | `tools/run-node-tests.sh --stage 71`  |
|    72 | `__filename` initialization and async `fs.readdir` errors                | `tools/run-node-tests.sh --stage 72`  |
|    73 | synchronous filesystem open validation and `ENOENT`                      | `tools/run-node-tests.sh --stage 73`  |
|    74 | filesystem stat/lstat/fstat metadata contracts                           | `tools/run-node-tests.sh --stage 74`  |
|    75 | stat options and missing-entry behavior                                  | `tools/run-node-tests.sh --stage 75`  |
|    76 | `Buffer.allocUnsafe` compatibility                                       | `tools/run-node-tests.sh --stage 76`  |
|    77 | binary Buffer-backed filesystem read/write                               | `tools/run-node-tests.sh --stage 77`  |
|    78 | AbortSignal-aware `fs.readFile` behavior                                 | `tools/run-node-tests.sh --stage 78`  |
|    79 | empty-file and encoding-aware `fs.readFile` behavior                     | `tools/run-node-tests.sh --stage 79`  |
|    80 | common platform skip flags                                               | `tools/run-node-tests.sh --stage 80`  |
|    81 | callback and Buffer-aware `fs.writeFile`                                 | `tools/run-node-tests.sh --stage 81`  |
|    82 | foundational promise-based filesystem write/read                         | `tools/run-node-tests.sh --stage 82`  |
|    83 | minimal `internal/test/binding` facade                                   | `tools/run-node-tests.sh --stage 83`  |
|    84 | `Buffer.equals` comparison                                               | `tools/run-node-tests.sh --stage 84`  |
|    85 | realpath and symlink capability helpers                                  | `tools/run-node-tests.sh --stage 85`  |
|    86 | copy-file flags and UV constants                                         | `tools/run-node-tests.sh --stage 86`  |
|    87 | copy-file path, mode, and callback validation                            | `tools/run-node-tests.sh --stage 87`  |
|    88 | async rename/rmdir and duplicate mkdir errors                            | `tools/run-node-tests.sh --stage 88`  |
|    89 | common invalid-argument diagnostic helper                                | `tools/run-node-tests.sh --stage 89`  |
|    90 | callback and Buffer-aware `fs.appendFile`                                | `tools/run-node-tests.sh --stage 90`  |
|    91 | asynchronous chmod and permission metadata                               | `tools/run-node-tests.sh --stage 91`  |
|    92 | fd-based `fchmod` compatibility                                          | `tools/run-node-tests.sh --stage 92`  |
|    93 | sync, async, and promise `fs.access`                                     | `tools/run-node-tests.sh --stage 93`  |
|    94 | `appendFileSync` data validation                                         | `tools/run-node-tests.sh --stage 94`  |
|    95 | foundational promise-based `appendFile`                                  | `tools/run-node-tests.sh --stage 95`  |
|    96 | sync/async `fs.statfs` metadata                                          | `tools/run-node-tests.sh --stage 96`  |
|    97 | sync/async/promise filesystem truncate                                   | `tools/run-node-tests.sh --stage 97`  |
|    98 | callback and promise `fs.rm`                                             | `tools/run-node-tests.sh --stage 98`  |
|    99 | file/directory-aware `fs.rmSync` behavior                                | `tools/run-node-tests.sh --stage 99`  |
|   100 | `fs.readdir` `withFileTypes` Dirent support                              | `tools/run-node-tests.sh --stage 100` |
|   101 | internal filesystem open-flag utility                                    | `tools/run-node-tests.sh --stage 101` |
|   102 | numeric and octal-string `fs.open` modes                                 | `tools/run-node-tests.sh --stage 102` |
|   103 | `fs.close` fd and callback validation                                    | `tools/run-node-tests.sh --stage 103` |
|   104 | fd mode-mask regression coverage                                         | `tools/run-node-tests.sh --stage 104` |
|   105 | promise-based readdir Dirents                                            | `tools/run-node-tests.sh --stage 105` |
|   106 | sync/async/promise directory handles                                     | `tools/run-node-tests.sh --stage 106` |
|   107 | async symlink and readlink callbacks                                     | `tools/run-node-tests.sh --stage 107` |
|   108 | symlink-aware `lstat` metadata                                           | `tools/run-node-tests.sh --stage 108` |
|   109 | promise-based symlink and readlink                                       | `tools/run-node-tests.sh --stage 109` |
|   110 | sync, callback, and promise `fs.unlink`                                  | `tools/run-node-tests.sh --stage 110` |
|   111 | sync, callback, and promise `fs.link`                                    | `tools/run-node-tests.sh --stage 111` |
|   112 | options-object `fs.read` sync/callback forms                             | `tools/run-node-tests.sh --stage 112` |
|   113 | Buffer and null-offset `fs.read` options                                 | `tools/run-node-tests.sh --stage 113` |
|   114 | `fs.read` offset and position validation                                 | `tools/run-node-tests.sh --stage 114` |
|   115 | sync, callback, and promise vectored `fs.readv`                          | `tools/run-node-tests.sh --stage 115` |
|   116 | sync write/read and vectored `fs.writev`                                 | `tools/run-node-tests.sh --stage 116` |
|   117 | `fs.open` write-mode truncation                                          | `tools/run-node-tests.sh --stage 117` |
|   118 | Buffer equality after vectored file writes                               | `tools/run-node-tests.sh --stage 118` |
|   119 | `fs.writev` invalid-buffer validation                                    | `tools/run-node-tests.sh --stage 119` |
|   120 | promise-based vectored `fs.readv`                                        | `tools/run-node-tests.sh --stage 120` |
|   121 | promise-based vectored `fs.writev`                                       | `tools/run-node-tests.sh --stage 121` |
|   122 | options-object callback `fs.write`                                       | `tools/run-node-tests.sh --stage 122` |
|   123 | `util.promisify` callback conversion                                     | `tools/run-node-tests.sh --stage 123` |
|   124 | promise file-handle buffer reads                                         | `tools/run-node-tests.sh --stage 124` |
|   125 | fd-based `fs.writeFileSync`                                              | `tools/run-node-tests.sh --stage 125` |
|   126 | typed-array `fs.writeFileSync` input                                     | `tools/run-node-tests.sh --stage 126` |
|   127 | DataView `fs.writeFileSync` input                                        | `tools/run-node-tests.sh --stage 127` |
|   128 | upstream common ArrayBuffer view helper                                  | `tools/run-node-tests.sh --stage 128` |
|   129 | `fs.writeFileSync` encoding and append options                           | `tools/run-node-tests.sh --stage 129` |
|   130 | `fs.writeFileSync` flush option                                          | `tools/run-node-tests.sh --stage 130` |
|   131 | fd-based callback `fs.appendFile`                                        | `tools/run-node-tests.sh --stage 131` |
|   132 | fd-based promise `fs.appendFile`                                         | `tools/run-node-tests.sh --stage 132` |
|   133 | sync `fs.ftruncate` size changes                                         | `tools/run-node-tests.sh --stage 133` |
|   134 | invalid callback `fs.truncate` lengths                                   | `tools/run-node-tests.sh --stage 134` |
|   135 | fractional `fs.truncateSync` lengths                                     | `tools/run-node-tests.sh --stage 135` |
|   136 | encoded synchronous `fs.readFileSync` results                            | `tools/run-node-tests.sh --stage 136` |
|   137 | buffer option on synchronous `fs.readFileSync`                           | `tools/run-node-tests.sh --stage 137` |
|   138 | create behavior of `fs.readFileSync` append mode                         | `tools/run-node-tests.sh --stage 138` |
|   139 | default permission mode for `fs.writeFileSync`                           | `tools/run-node-tests.sh --stage 139` |
|   140 | promise file-handle offset writes                                        | `tools/run-node-tests.sh --stage 140` |
|   141 | promise file-handle vectored read/write                                  | `tools/run-node-tests.sh --stage 141` |
|   142 | promise file-handle truncation                                           | `tools/run-node-tests.sh --stage 142` |
|   143 | promise file-handle metadata and sync methods                            | `tools/run-node-tests.sh --stage 143` |
|   144 | file-handle chmod and closed-handle errors                               | `tools/run-node-tests.sh --stage 144` |
|   145 | promise file-handle read/write file methods                              | `tools/run-node-tests.sh --stage 145` |
|   146 | promise file-handle append file method                                   | `tools/run-node-tests.sh --stage 146` |
|   147 | native synchronous `fs.realpath`                                         | `tools/run-node-tests.sh --stage 147` |
|   148 | encoded synchronous `fs.realpath` results                                | `tools/run-node-tests.sh --stage 148` |
|   149 | upstream common fixtures directory                                       | `tools/run-node-tests.sh --stage 149` |
|   150 | promise `fs.realpath` and Buffer encoding                                | `tools/run-node-tests.sh --stage 150` |
|   151 | promise fd metadata and permission methods                               | `tools/run-node-tests.sh --stage 151` |
|   152 | synchronous and callback `fs.access` validation                          | `tools/run-node-tests.sh --stage 152` |
|   153 | promise `fs.chmod` permission changes                                    | `tools/run-node-tests.sh --stage 153` |
|   154 | promise copy, rename, and unlink mutations                               | `tools/run-node-tests.sh --stage 154` |
|   155 | promise directory removal                                                | `tools/run-node-tests.sh --stage 155` |
|   156 | promise temporary-directory creation                                     | `tools/run-node-tests.sh --stage 156` |
|   157 | promise symbolic-link metadata with `lstat`                              | `tools/run-node-tests.sh --stage 157` |
|   158 | synchronous, callback, and promise hard links                            | `tools/run-node-tests.sh --stage 158` |
|   159 | invalid source/target validation for `fs.linkSync`                       | `tools/run-node-tests.sh --stage 159` |
|   160 | encoded synchronous `fs.readlink` results                                | `tools/run-node-tests.sh --stage 160` |
|   161 | upstream symlink capability helper                                       | `tools/run-node-tests.sh --stage 161` |
|   162 | invalid target validation for `fs.symlinkSync`                           | `tools/run-node-tests.sh --stage 162` |
|   163 | invalid type validation for `fs.symlinkSync`                             | `tools/run-node-tests.sh --stage 163` |
|   164 | promise file-handle basic `writeFile`                                    | `tools/run-node-tests.sh --stage 164` |
|   165 | promise file-handle iterable `writeFile`                                 | `tools/run-node-tests.sh --stage 165` |
|   166 | promise file-handle async-iterable `writeFile`                           | `tools/run-node-tests.sh --stage 166` |
|   167 | promise file-handle `writeFile` validation                               | `tools/run-node-tests.sh --stage 167` |
|   168 | promise file-handle `writeFile` encoding                                 | `tools/run-node-tests.sh --stage 168` |
|   169 | file-handle pull text and byte iterators                                 | `tools/run-node-tests.sh --stage 169` |
|   170 | file-handle pull start/limit/chunk options                               | `tools/run-node-tests.sh --stage 170` |
|   171 | file-handle pull locking and position state                              | `tools/run-node-tests.sh --stage 171` |
|   172 | file-handle pull transforms and abort signals                            | `tools/run-node-tests.sh --stage 172` |
|   173 | file-handle pull option validation                                       | `tools/run-node-tests.sh --stage 173` |
|   174 | file-handle pull stream transform modules                                | `tools/run-node-tests.sh --stage 174` |
|   175 | file-handle pull batch chunking                                          | `tools/run-node-tests.sh --stage 175` |
|   176 | file-handle `readFile` and abort signals                                 | `tools/run-node-tests.sh --stage 176` |
|   177 | file-handle `write` contract and validation                              | `tools/run-node-tests.sh --stage 177` |
|   178 | callback `realpath` contract and errors                                  | `tools/run-node-tests.sh --stage 178` |
|   179 | path parse/format namespaces and validation                              | `tools/run-node-tests.sh --stage 179` |
|   180 | Win32 path parse/format and basename behavior                            | `tools/run-node-tests.sh --stage 180` |
|   181 | `util.format` primitive and placeholder behavior                         | `tools/run-node-tests.sh --stage 181` |
|   182 | `util.inspect` defaults and format options                               | `tools/run-node-tests.sh --stage 182` |
|   183 | `util.format` BigInt conversions                                         | `tools/run-node-tests.sh --stage 183` |
|   184 | `util.format` symbol numeric conversions                                 | `tools/run-node-tests.sh --stage 184` |
|   185 | `util.format` numeric separators and options                             | `tools/run-node-tests.sh --stage 185` |
|   186 | `util.format` string objects and custom conversion                       | `tools/run-node-tests.sh --stage 186` |
|   187 | Buffer hex writes and invalid input truncation                           | `tools/run-node-tests.sh --stage 187` |
|   188 | Buffer `includes` values, offsets, and encodings                         | `tools/run-node-tests.sh --stage 188` |
|   189 | Buffer `includes` argument validation                                    | `tools/run-node-tests.sh --stage 189` |
|   190 | Buffer double-precision read/write and byte order                        | `tools/run-node-tests.sh --stage 190` |
|   191 | Buffer unsigned integer read/write and bounds errors                     | `tools/run-node-tests.sh --stage 191` |
|   192 | Buffer variable-width unsigned integer operations                        | `tools/run-node-tests.sh --stage 192` |
|   193 | Buffer Uint/UInt method aliases                                          | `tools/run-node-tests.sh --stage 193` |
|   194 | Buffer variable-width read validation                                    | `tools/run-node-tests.sh --stage 194` |
|   195 | Buffer signed integer read/write operations                              | `tools/run-node-tests.sh --stage 195` |
|   196 | Buffer single-precision float read/write operations                      | `tools/run-node-tests.sh --stage 196` |
|   197 | Buffer slicing, callable construction, and comparison                    | `tools/run-node-tests.sh --stage 197` |
|   198 | Buffer copy ranges, counts, and overlap semantics                        | `tools/run-node-tests.sh --stage 198` |
|   199 | Buffer concatenation, limits, and zero filling                           | `tools/run-node-tests.sh --stage 199` |
|   200 | Buffer fill strings, encodings, and numeric values                       | `tools/run-node-tests.sh --stage 200` |
|   201 | Buffer index search, offsets, and missing values                         | `tools/run-node-tests.sh --stage 201` |
|   202 | Buffer JSON shape and round-trip restoration                             | `tools/run-node-tests.sh --stage 202` |
|   203 | Buffer equality, typed arrays, and validation                            | `tools/run-node-tests.sh --stage 203` |
|   204 | Buffer byte lengths across encodings and views                           | `tools/run-node-tests.sh --stage 204` |
|   205 | Buffer string writes, offsets, and UTF-16 encoding                       | `tools/run-node-tests.sh --stage 205` |
|   206 | Buffer instance/static comparison and ordering                           | `tools/run-node-tests.sh --stage 206` |
|   207 | Buffer factory constructors and unsafe allocation                        | `tools/run-node-tests.sh --stage 207` |
|   208 | Buffer endian swap operations                                            | `tools/run-node-tests.sh --stage 208` |
|   209 | Buffer BigInt read/write operations                                      | `tools/run-node-tests.sh --stage 209` |
|   210 | Buffer ArrayBuffer views and memory sharing                              | `tools/run-node-tests.sh --stage 210` |
|   211 | Buffer `isEncoding` validation                                           | `tools/run-node-tests.sh --stage 211` |
|   212 | Buffer `copyBytesFrom` typed-array copies                                | `tools/run-node-tests.sh --stage 212` |
|   213 | `vm.runInNewContext` evaluation shim                                     | `tools/run-node-tests.sh --stage 213` |
|   214 | Buffer `from` string coercion                                            | `tools/run-node-tests.sh --stage 214` |
|   215 | Buffer `from` argument validation                                        | `tools/run-node-tests.sh --stage 215` |
|   216 | Buffer constants and `kMaxLength`                                        | `tools/run-node-tests.sh --stage 216` |
|   217 | Buffer backing metadata and pool size                                    | `tools/run-node-tests.sh --stage 217` |
|   218 | Buffer `toString` encoding case-insensitivity                            | `tools/run-node-tests.sh --stage 218` |
|   219 | Buffer `write` unknown-encoding validation                               | `tools/run-node-tests.sh --stage 219` |
|   220 | Buffer `write` overload validation                                       | `tools/run-node-tests.sh --stage 220` |
|   221 | Buffer `toString` offset/range clamping                                  | `tools/run-node-tests.sh --stage 221` |
|   222 | Buffer `from` encoding conversions (ascii, utf-16le)                     | `tools/run-node-tests.sh --stage 222` |
|   223 | Buffer base64 whitespace tolerance                                       | `tools/run-node-tests.sh --stage 223` |
|   224 | Buffer base64 invalid-input handling                                     | `tools/run-node-tests.sh --stage 224` |
|   225 | Buffer `inspect` hex rendering                                           | `tools/run-node-tests.sh --stage 225` |
|   226 | Buffer `write` UTF-8 partial-char handling                               | `tools/run-node-tests.sh --stage 226` |
|   227 | Buffer lone-surrogate UTF-8 replacement                                  | `tools/run-node-tests.sh --stage 227` |
|   228 | Buffer `alloc` encoded fill values                                       | `tools/run-node-tests.sh --stage 228` |
|   229 | Buffer `from` unknown-encoding validation                                | `tools/run-node-tests.sh --stage 229` |
|   230 | Buffer prototype metadata and alloc edge cases                           | `tools/run-node-tests.sh --stage 230` |
|   231 | `crypto.randomBytes` and `randomFillSync`                                | `tools/run-node-tests.sh --stage 231` |
|   232 | Buffer `isAscii`/`isUtf8` predicates                                     | `tools/run-node-tests.sh --stage 232` |
|   233 | `util` TextEncoder/TextDecoder exports                                   | `tools/run-node-tests.sh --stage 233` |
|   234 | `internal/errors` code classes                                           | `tools/run-node-tests.sh --stage 234` |
|   235 | Buffer `fill` invalid hex validation                                     | `tools/run-node-tests.sh --stage 235` |
|   236 | Buffer `fill` null coercion                                              | `tools/run-node-tests.sh --stage 236` |
|   237 | Buffer `fill` range argument validation                                  | `tools/run-node-tests.sh --stage 237` |
|   238 | internal `arrayBufferViewHasBuffer` marker                               | `tools/run-node-tests.sh --stage 238` |
|   239 | internal lazy arrayBuffer backing state                                  | `tools/run-node-tests.sh --stage 239` |
|   240 | Buffer `allocUnsafeSlow` argument validation                             | `tools/run-node-tests.sh --stage 240` |
|   241 | Buffer `toString` end-range edge clamping                                | `tools/run-node-tests.sh --stage 241` |
|   242 | Buffer BigInt read/write alias parity                                    | `tools/run-node-tests.sh --stage 242` |
|   243 | Buffer display hooks (`toLocaleString`, inspect symbol)                  | `tools/run-node-tests.sh --stage 243` |
|   244 | `internal/buffer` `utf8Write` export                                     | `tools/run-node-tests.sh --stage 244` |
|   245 | Buffer size fractional truncation                                        | `tools/run-node-tests.sh --stage 245` |
|   246 | Buffer `indexOf` UCS-2 odd-offset handling                               | `tools/run-node-tests.sh --stage 246` |
|   247 | Buffer `indexOf` encoding overload                                       | `tools/run-node-tests.sh --stage 247` |
|   248 | `util.inspect` Buffer rendering                                          | `tools/run-node-tests.sh --stage 248` |
|   249 | Buffer `INSPECT_MAX_BYTES` live limit                                    | `tools/run-node-tests.sh --stage 249` |
|   250 | Buffer inspect own-property rendering                                    | `tools/run-node-tests.sh --stage 250` |
|   251 | `util.format` symbol numeric conversions                                 | `tools/run-node-tests.sh --stage 251` |
|   252 | internal `JSStream` binding shim                                         | `tools/run-node-tests.sh --stage 252` |
|   253 | `util.types` basic predicates                                            | `tools/run-node-tests.sh --stage 253` |
|   254 | `util.types` full predicate matrix                                       | `tools/run-node-tests.sh --stage 254` |
|   255 | typed-array brand checks (DataView vs TypedArray)                        | `tools/run-node-tests.sh --stage 255` |
|   256 | `util.types` vm module namespace and key checks                          | `tools/run-node-tests.sh --stage 256` |
|   257 | Buffer ascii byte conversion                                             | `tools/run-node-tests.sh --stage 257` |
|   258 | Buffer detached-arraybuffer state validation                             | `tools/run-node-tests.sh --stage 258` |
|   259 | Buffer surrogate-pair UTF-8 encoding                                     | `tools/run-node-tests.sh --stage 259` |
|   260 | Buffer `compare` offset-selection and validation                         | `tools/run-node-tests.sh --stage 260` |
|   261 | Buffer `fill` forged-length bounds check                                 | `tools/run-node-tests.sh --stage 261` |
|   262 | Buffer `copyBytesFrom` element-size bounds                               | `tools/run-node-tests.sh --stage 262` |
|   263 | Buffer `concat` length and truncation                                    | `tools/run-node-tests.sh --stage 263` |
|   264 | Buffer inspect limit and named-property rendering                        | `tools/run-node-tests.sh --stage 264` |
|   276 | `util.format` float and numeric edge cases                               | `tools/run-node-tests.sh --stage 276` |
|   277 | `util.inspect` string and function rendering                             | `tools/run-node-tests.sh --stage 277` |
|   278 | `node:test` options callback invocation                                  | `tools/run-node-tests.sh --stage 278` |
|   279 | `url.format` string and legacy object output                             | `tools/run-node-tests.sh --stage 279` |
|   280 | `url.parse` component extraction and validation                          | `tools/run-node-tests.sh --stage 280` |
|   281 | `url.parse` error codes and invalid inputs                               | `tools/run-node-tests.sh --stage 281` |
|   282 | `url.parse` normalization and backslash handling                         | `tools/run-node-tests.sh --stage 282` |
|   283 | `querystring.parse`/`stringify` core behavior                            | `tools/run-node-tests.sh --stage 283` |
|   284 | `querystring.stringify` object value coercion                            | `tools/run-node-tests.sh --stage 284` |
|   285 | `querystring` encoder options and URI errors                             | `tools/run-node-tests.sh --stage 285` |
|   286 | `querystring` numeric coercion and `maxKeys`                             | `tools/run-node-tests.sh --stage 286` |
|   287 | `querystring` decoding, `unescapeBuffer`, and options                    | `tools/run-node-tests.sh --stage 287` |
|   288 | `querystring` decode fallback and `unescape` override                    | `tools/run-node-tests.sh --stage 288` |
|   289 | `querystring.unescape` malformed-escape handling                         | `tools/run-node-tests.sh --stage 289` |
|   290 | `querystring` upstream-complete behavior                                 | `tools/run-node-tests.sh --stage 290` |
|   291 | `util.format` null-prototype object rendering                            | `tools/run-node-tests.sh --stage 291` |
|   292 | `util.format` class null-prototype rendering                             | `tools/run-node-tests.sh --stage 292` |
|   293 | `util.format` class instance rendering                                   | `tools/run-node-tests.sh --stage 293` |
|   294 | `util.format` Array subclass rendering                                   | `tools/run-node-tests.sh --stage 294` |
|   295 | `util.format` Symbol.toPrimitive conversion                              | `tools/run-node-tests.sh --stage 295` |
|   296 | `util.format` Date and Symbol built-in rendering                         | `tools/run-node-tests.sh --stage 296` |
|   297 | `util.format` Date string conversion                                     | `tools/run-node-tests.sh --stage 297` |
|   298 | `util.format` object string inspection and escaping                      | `tools/run-node-tests.sh --stage 298` |
|   299 | POSIX path invalid argument validation                                   | `tools/run-node-tests.sh --stage 299` |
|   300 | Win32 path invalid argument validation                                   | `tools/run-node-tests.sh --stage 300` |
|   301 | Path basename suffix validation and removal                              | `tools/run-node-tests.sh --stage 301` |
|   302 | POSIX/Win32 path separators and delimiters                               | `tools/run-node-tests.sh --stage 302` |
|   303 | POSIX/Win32 path upstream-complete normalization                         | `tools/run-node-tests.sh --stage 303` |
|   304 | Win32 path literal normalization and basename                            | `tools/run-node-tests.sh --stage 304` |
|   305 | Win32 path slash-root parsing                                            | `tools/run-node-tests.sh --stage 305` |
|   306 | POSIX path trailing-separator parsing                                    | `tools/run-node-tests.sh --stage 306` |
|   307 | POSIX path dirname root and relative behavior                            | `tools/run-node-tests.sh --stage 307` |
|   308 | POSIX path trailing-separator dirname                                    | `tools/run-node-tests.sh --stage 308` |
|   309 | Win32 UNC path parsing                                                   | `tools/run-node-tests.sh --stage 309` |
|   310 | OS uptime and process priority APIs                                      | `tools/run-node-tests.sh --stage 310` |
|   311 | OS positive uptime contract                                              | `tools/run-node-tests.sh --stage 311` |
|   312 | OS host information and loopback interfaces                              | `tools/run-node-tests.sh --stage 312` |
|   313 | OS user information and Buffer encoding                                  | `tools/run-node-tests.sh --stage 313` |
|   314 | OS devNull and available parallelism APIs                                | `tools/run-node-tests.sh --stage 314` |
|   315 | OS function numeric coercion                                             | `tools/run-node-tests.sh --stage 315` |
|   316 | OS core metrics upstream-complete contract                               | `tools/run-node-tests.sh --stage 316` |
|   317 | common.mustCallAtLeast callback counting                                 | `tools/run-node-tests.sh --stage 317` |
|   318 | OS tmpdir environment precedence and normalization                       | `tools/run-node-tests.sh --stage 318` |
|   319 | POSIX os.tmpdir slash preservation                                       | `tools/run-node-tests.sh --stage 319` |
|   320 | OS string-returning function coercion                                    | `tools/run-node-tests.sh --stage 320` |
|   321 | OS totalmem numeric coercion                                             | `tools/run-node-tests.sh --stage 321` |
|   322 | OS upstream-complete core contract                                       | `tools/run-node-tests.sh --stage 322` |
|   323 | util core compatibility helpers                                          | `tools/run-node-tests.sh --stage 323` |
|   324 | util error and native error validation                                   | `tools/run-node-tests.sh --stage 324` |
|   325 | internal IPC error code compatibility                                    | `tools/run-node-tests.sh --stage 325` |
|   326 | util upstream-complete contract                                          | `tools/run-node-tests.sh --stage 326` |
|   327 | util format trailing arguments and object inspection                     | `tools/run-node-tests.sh --stage 327` |
|   328 | util format detailed object inspection                                   | `tools/run-node-tests.sh --stage 328` |
|   329 | util format uppercase object inspection                                  | `tools/run-node-tests.sh --stage 329` |
|   330 | util format nested array inspection                                      | `tools/run-node-tests.sh --stage 330` |
|   331 | util format nested object inspection                                     | `tools/run-node-tests.sh --stage 331` |
|   332 | util format Error stack rendering                                        | `tools/run-node-tests.sh --stage 332` |
|   333 | util format CSS directive handling                                       | `tools/run-node-tests.sh --stage 333` |
|   334 | util format JSON circular handling                                       | `tools/run-node-tests.sh --stage 334` |
|   335 | util formatWithOptions color handling                                    | `tools/run-node-tests.sh --stage 335` |
|   336 | util formatWithOptions compact handling                                  | `tools/run-node-tests.sh --stage 336` |
|   337 | util format SharedArrayBuffer inspection                                 | `tools/run-node-tests.sh --stage 337` |
|   338 | util format custom Error rendering                                       | `tools/run-node-tests.sh --stage 338` |
|   339 | util formatWithOptions invalid options validation                        | `tools/run-node-tests.sh --stage 339` |
|   340 | common non-debug util type regression                                    | `tools/run-node-tests.sh --stage 340` |
|   341 | process stream TTY flags                                                 | `tools/run-node-tests.sh --stage 341` |
|   342 | AssertionError inheritance and throwing                                  | `tools/run-node-tests.sh --stage 342` |
|   343 | vm context creation and evaluation                                       | `tools/run-node-tests.sh --stage 343` |
|   344 | assert Error object message handling                                     | `tools/run-node-tests.sh --stage 344` |
|   345 | assert.throws return value and constructor matching                      | `tools/run-node-tests.sh --stage 345` |
|   346 | assert.throws object matching                                            | `tools/run-node-tests.sh --stage 346` |
|   347 | assert message formatting and operators                                  | `tools/run-node-tests.sh --stage 347` |
|   348 | assert.throws constructor mismatch                                       | `tools/run-node-tests.sh --stage 348` |
|   349 | AssertionError generated metadata                                        | `tools/run-node-tests.sh --stage 349` |
|   350 | assert.doesNotThrow failure handling                                     | `tools/run-node-tests.sh --stage 350` |
|   351 | assert.throws regex and function validators                              | `tools/run-node-tests.sh --stage 351` |
|   352 | assert.throws regex mismatch handling                                    | `tools/run-node-tests.sh --stage 352` |
|   353 | assert strict reference mismatch                                         | `tools/run-node-tests.sh --stage 353` |
|   354 | assert missing expected exception handling                               | `tools/run-node-tests.sh --stage 354` |
|   355 | assert Error object reference mismatch                                   | `tools/run-node-tests.sh --stage 355` |
|   356 | assert.throws custom message formatting                                  | `tools/run-node-tests.sh --stage 356` |
|   357 | Buffer numeric read methods                                              | `tools/run-node-tests.sh --stage 357` |
|   358 | Buffer write range validation                                            | `tools/run-node-tests.sh --stage 358` |
|   359 | Buffer write encoding support                                            | `tools/run-node-tests.sh --stage 359` |
|   360 | Buffer comparison methods                                                | `tools/run-node-tests.sh --stage 360` |
|   361 | Buffer fill encoding validation                                          | `tools/run-node-tests.sh --stage 361` |
|   362 | Buffer binary sharing and composition                                    | `tools/run-node-tests.sh --stage 362` |
|   363 | fs Buffer binary round-trip                                              | `tools/run-node-tests.sh --stage 363` |
|   364 | fs positioned Buffer reads and writes                                    | `tools/run-node-tests.sh --stage 364` |
|   365 | fs append Buffer bytes                                                   | `tools/run-node-tests.sh --stage 365` |
|   366 | timer timeout scheduling and cancellation                                | `tools/run-node-tests.sh --stage 366` |
|   367 | timer interval repetition and cancellation                               | `tools/run-node-tests.sh --stage 367` |
|   368 | timer callback arguments and immediate handles                           | `tools/run-node-tests.sh --stage 368` |
|   369 | process.nextTick callback arguments                                      | `tools/run-node-tests.sh --stage 369` |
|   370 | stream writable backpressure and drain                                   | `tools/run-node-tests.sh --stage 370` |
|   371 | stream readable pause and resume                                         | `tools/run-node-tests.sh --stage 371` |
|   372 | stream Transform chunk processing                                        | `tools/run-node-tests.sh --stage 372` |
|   373 | stream pipe backpressure handling                                        | `tools/run-node-tests.sh --stage 373` |
|   374 | crypto hash Buffer input and hex digest                                  | `tools/run-node-tests.sh --stage 374` |
|   375 | crypto random byte generation and filling                                | `tools/run-node-tests.sh --stage 375` |
|   376 | fs ENOENT error metadata                                                 | `tools/run-node-tests.sh --stage 376` |
|   377 | fs invalid path argument validation                                      | `tools/run-node-tests.sh --stage 377` |
|   378 | fs invalid write path validation                                         | `tools/run-node-tests.sh --stage 378` |
|   379 | fs unknown encoding validation                                           | `tools/run-node-tests.sh --stage 379` |
|   380 | crypto HMAC Buffer input and hex digest                                  | `tools/run-node-tests.sh --stage 380` |
|   381 | crypto digest Buffer output                                              | `tools/run-node-tests.sh --stage 381` |
|   382 | stream finish event before end callback                                  | `tools/run-node-tests.sh --stage 382` |
|   383 | isolated runtime state across stage fixtures                             | `tools/run-node-tests.sh --stage 383` |
|   384 | crypto module bootstrap and randomUUID availability                      | `tools/run-node-tests.sh --stage 384` |
|   385 | lazy crypto bootstrap state initialization                               | `tools/run-node-tests.sh --stage 385` |
|   386 | lazy stream bootstrap state initialization                               | `tools/run-node-tests.sh --stage 386` |
|   387 | fs close invalid descriptor EBADF metadata                               | `tools/run-node-tests.sh --stage 387` |
|   388 | lazy URL bootstrap and url.parse availability                            | `tools/run-node-tests.sh --stage 388` |
|   389 | lazy OS bootstrap and os.platform availability                           | `tools/run-node-tests.sh --stage 389` |
|   390 | fs close callback descriptor release                                     | `tools/run-node-tests.sh --stage 390` |
|   391 | crypto PBKDF2 sync and callback derivation                               | `tools/run-node-tests.sh --stage 391` |
|   392 | crypto PBKDF2 argument validation and error codes                        | `tools/run-node-tests.sh --stage 392` |
|   393 | lazy querystring bootstrap and stringify availability                    | `tools/run-node-tests.sh --stage 393` |
|   394 | experimental stream/iter feature flag gating                             | `tools/run-node-tests.sh --stage 394` |
|   395 | crypto advertised hash and cipher capabilities                           | `tools/run-node-tests.sh --stage 395` |
|   396 | crypto timingSafeEqual comparison and length errors                      | `tools/run-node-tests.sh --stage 396` |
|   397 | crypto randomInt ranges validation and callback                          | `tools/run-node-tests.sh --stage 397` |
|   398 | crypto randomFill offset range and buffer return                         | `tools/run-node-tests.sh --stage 398` |
|   399 | crypto hash and HMAC base64 digests                                      | `tools/run-node-tests.sh --stage 399` |
|   400 | crypto hash and HMAC update input encoding                               | `tools/run-node-tests.sh --stage 400` |
|   401 | timers/promises setTimeout delay and value                               | `tools/run-node-tests.sh --stage 401` |
|   402 | perf_hooks performance clock and serialization                           | `tools/run-node-tests.sh --stage 402` |
|   403 | perf_hooks user timing marks and measures                                | `tools/run-node-tests.sh --stage 403` |
|   404 | perf_hooks entry queries and measure clearing                            | `tools/run-node-tests.sh --stage 404` |
|   405 | timers/promises async interval iteration and values                      | `tools/run-node-tests.sh --stage 405` |
|   406 | timers/promises AbortSignal cancellation error                           | `tools/run-node-tests.sh --stage 406` |
|   407 | timers/promises interval AbortSignal cancellation                        | `tools/run-node-tests.sh --stage 407` |
|   408 | timers/promises immediate AbortSignal cancellation                       | `tools/run-node-tests.sh --stage 408` |
|   409 | process platform, memory, and resource metadata                          | `tools/run-node-tests.sh --stage 409` |
|   410 | process.binding unknown module error                                     | `tools/run-node-tests.sh --stage 410` |
|   411 | process.getBuiltinModule known and unknown modules                       | `tools/run-node-tests.sh --stage 411` |
|   412 | perf_hooks timerify thrown error preservation                            | `tools/run-node-tests.sh --stage 412` |
|   413 | fs FileHandle read position and readFile continuation                    | `tools/run-node-tests.sh --stage 413` |
|   414 | fs FileHandle readFile string encoding                                   | `tools/run-node-tests.sh --stage 414` |
|   415 | fs readFileSync file descriptor input                                    | `tools/run-node-tests.sh --stage 415` |
|   416 | fs appendFile synchronous data validation                                | `tools/run-node-tests.sh --stage 416` |
|   417 | fs appendFileSync binary Buffer preservation                             | `tools/run-node-tests.sh --stage 417` |
|   418 | fs appendFileSync hex and base64 string encodings                        | `tools/run-node-tests.sh --stage 418` |
|   419 | fs statSync missing path ENOENT metadata                                 | `tools/run-node-tests.sh --stage 419` |
|   420 | crypto hash and HMAC copy state branching                                | `tools/run-node-tests.sh --stage 420` |
|   421 | crypto finalized hash and HMAC operation errors                          | `tools/run-node-tests.sh --stage 421` |
|   422 | fs WriteStream open, end, and close lifecycle                            | `tools/run-node-tests.sh --stage 422` |
|   423 | fs ReadStream data delivery and close lifecycle                          | `tools/run-node-tests.sh --stage 423` |
|   424 | fs ReadStream encoding options and bytesRead                             | `tools/run-node-tests.sh --stage 424` |
|   425 | fs ReadStream inverted range validation                                  | `tools/run-node-tests.sh --stage 425` |
|   426 | fs WriteStream encoding options and bytesWritten                         | `tools/run-node-tests.sh --stage 426` |
|   427 | fs ReadStream close descriptor release                                   | `tools/run-node-tests.sh --stage 427` |
|   428 | fs WriteStream close descriptor release                                  | `tools/run-node-tests.sh --stage 428` |
|   429 | process exit event ordering and zero exit code                           | `tools/run-node-tests.sh --stage 429` |
|   430 | fs stream autoClose false descriptor retention                           | `tools/run-node-tests.sh --stage 430` |
|   431 | fs WriteStream autoClose false descriptor retention                      | `tools/run-node-tests.sh --stage 431` |
|   432 | stream destroy error, close, and destroyed state                         | `tools/run-node-tests.sh --stage 432` |
|   433 | process nextTick ordering before promise callbacks                       | `tools/run-node-tests.sh --stage 433` |
|   434 | process nextTick callback validation                                     | `tools/run-node-tests.sh --stage 434` |
|   435 | timer callback validation across timeout/immediate/interval              | `tools/run-node-tests.sh --stage 435` |
|   436 | timer handle ref, unref, and hasRef state                                | `tools/run-node-tests.sh --stage 436` |
|   437 | stream isPaused pause and resume state                                   | `tools/run-node-tests.sh --stage 437` |
|   438 | stream writableNeedDrain backpressure and drain clearing                 | `tools/run-node-tests.sh --stage 438` |
|   439 | stream readable and writable completion flags                            | `tools/run-node-tests.sh --stage 439` |
|   440 | stream readable and writable state after destroy                         | `tools/run-node-tests.sh --stage 440` |
|   441 | stream writable cork nesting and uncork state                            | `tools/run-node-tests.sh --stage 441` |
|   442 | stream cork buffering and uncork emission                                | `tools/run-node-tests.sh --stage 442` |
|   443 | fs WriteStream append flags preserve existing content                    | `tools/run-node-tests.sh --stage 443` |
|   444 | stream Readable push chunks and end signaling                            | `tools/run-node-tests.sh --stage 444` |
|   445 | stream Readable unshift chunk delivery                                   | `tools/run-node-tests.sh --stage 445` |
|   446 | stream Readable read sizes and FIFO queue                                | `tools/run-node-tests.sh --stage 446` |
|   447 | stream Readable EOF ordering after buffered read                         | `tools/run-node-tests.sh --stage 447` |
|   448 | stream iterator consumption after data listeners                         | `tools/run-node-tests.sh --stage 448` |
|   449 | stream Readable unshift EOF and buffered body                            | `tools/run-node-tests.sh --stage 449` |
|   450 | stream Readable default Buffer queue and draining                        | `tools/run-node-tests.sh --stage 450` |
|   451 | stream flow transition drains queued data and EOF                        | `tools/run-node-tests.sh --stage 451` |
|   452 | stream resume drains paused queued data                                  | `tools/run-node-tests.sh --stage 452` |
|   453 | stream writable length recovery without spurious drain                   | `tools/run-node-tests.sh --stage 453` |
|   454 | stream writable end chainability and finish event                        | `tools/run-node-tests.sh --stage 454` |
|   455 | stream write-after-end rejection and error code                          | `tools/run-node-tests.sh --stage 455` |
|   456 | stream push-after-EOF rejection and error code                           | `tools/run-node-tests.sh --stage 456` |
|   457 | stream unshift-after-end rejection and error code                        | `tools/run-node-tests.sh --stage 457` |
|   458 | stream Readable read(0) preserves queued data                            | `tools/run-node-tests.sh --stage 458` |
|   459 | stream readableEnded changes only after end                              | `tools/run-node-tests.sh --stage 459` |
|   460 | stream readable event exposes queued data                                | `tools/run-node-tests.sh --stage 460` |
|   461 | stream late readable listener drains queued data                         | `tools/run-node-tests.sh --stage 461` |
|   462 | stream readableLength tracks partial buffer reads                        | `tools/run-node-tests.sh --stage 462` |
|   463 | stream readableFlowing tracks pause and resume state                     | `tools/run-node-tests.sh --stage 463` |
|   464 | stream readable and writable object mode flags                           | `tools/run-node-tests.sh --stage 464` |
|   465 | stream destroy callback receives original error                          | `tools/run-node-tests.sh --stage 465` |
|   466 | stream writable destroy callback receives original error                 | `tools/run-node-tests.sh --stage 466` |
|   467 | stream Readable.from uses object mode                                    | `tools/run-node-tests.sh --stage 467` |
|   468 | timer handle ref and unref state                                         | `tools/run-node-tests.sh --stage 468` |
|   469 | timer handle refresh is chainable                                        | `tools/run-node-tests.sh --stage 469` |
|   470 | timer refresh after clear reactivates once                               | `tools/run-node-tests.sh --stage 470` |
|   471 | interval refresh after clear reactivates once                            | `tools/run-node-tests.sh --stage 471` |
|   472 | stream setEncoding chains and decodes queued buffers                     | `tools/run-node-tests.sh --stage 472` |
|   473 | stream setEncoding decodes flowing data                                  | `tools/run-node-tests.sh --stage 473` |
|   474 | stream write after destroy rejects with error                            | `tools/run-node-tests.sh --stage 474` |
|   475 | stream push after destroy rejects with error                             | `tools/run-node-tests.sh --stage 475` |
|   476 | stream writableLength counts UTF-8 bytes                                 | `tools/run-node-tests.sh --stage 476` |
|   477 | stream read combines queued buffer chunks                                | `tools/run-node-tests.sh --stage 477` |
|   478 | stream push string queues byte data                                      | `tools/run-node-tests.sh --stage 478` |
|   479 | fs readFile requires a callback                                          | `tools/run-node-tests.sh --stage 479` |
|   480 | fs mkdtemp requires a callback                                           | `tools/run-node-tests.sh --stage 480` |
|   481 | fs mkdtempSync validates prefix type                                     | `tools/run-node-tests.sh --stage 481` |
|   482 | fs async mkdtemp validates prefix type                                   | `tools/run-node-tests.sh --stage 482` |
|   483 | fs mkdtemp validates options type                                        | `tools/run-node-tests.sh --stage 483` |
|   484 | crypto HMAC digest validates encoding                                    | `tools/run-node-tests.sh --stage 484` |
|   485 | crypto hash digest validates encoding                                    | `tools/run-node-tests.sh --stage 485` |
|   486 | crypto finalized hash copy rejects                                       | `tools/run-node-tests.sh --stage 486` |
|   487 | crypto randomBytes validates size and callback                           | `tools/run-node-tests.sh --stage 487` |
|   488 | crypto randomFillSync validates buffer and range                         | `tools/run-node-tests.sh --stage 488` |
|   489 | querystring rejects lone surrogate encoding                              | `tools/run-node-tests.sh --stage 489` |
|   490 | querystring preserves Unicode values                                     | `tools/run-node-tests.sh --stage 490` |
|   491 | querystring.parse uses writable unescape hook                            | `tools/run-node-tests.sh --stage 491` |
|   492 | internal event target exposes weak handler symbol                        | `tools/run-node-tests.sh --stage 492` |
|   493 | async_hooks exposes resource and chainable hooks                         | `tools/run-node-tests.sh --stage 493` |
|   494 | http server and client exchange JSON in process                          | `tools/run-node-tests.sh --stage 494` |
|   495 | async resource state propagates through timers                           | `tools/run-node-tests.sh --stage 495` |
|   496 | async resource bind preserves receiver and metadata                      | `tools/run-node-tests.sh --stage 496` |
|   497 | buffer legacy encoding methods are exposed                               | `tools/run-node-tests.sh --stage 497` |
|   498 | buffer prototype surface omits underscored methods                       | `tools/run-node-tests.sh --stage 498` |
|   499 | buffer inspect hook formats generic typed arrays                         | `tools/run-node-tests.sh --stage 499` |
|   500 | buffer float writes validate integer and range offsets                   | `tools/run-node-tests.sh --stage 500` |
|   501 | child process exit reports code and null signal                          | `tools/run-node-tests.sh --stage 501` |
|   502 | child process IPC send reports queue backpressure                        | `tools/run-node-tests.sh --stage 502` |
|   503 | child process eval exit reports normal completion                        | `tools/run-node-tests.sh --stage 503` |
|   504 | cluster primary exposes worker lifecycle controls                        | `tools/run-node-tests.sh --stage 504` |
|   505 | cluster setup emits configured settings                                  | `tools/run-node-tests.sh --stage 505` |
|   506 | common child-process helper validates spawn results                      | `tools/run-node-tests.sh --stage 506` |
|   507 | cluster worker lifecycle and listening metadata                          | `tools/run-node-tests.sh --stage 507` |
|   508 | cluster fork merges custom environment variables                         | `tools/run-node-tests.sh --stage 508` |
|   509 | cluster disconnect events report clean worker exit                       | `tools/run-node-tests.sh --stage 509` |
|   510 | cluster kill reports terminating signal and state                        | `tools/run-node-tests.sh --stage 510` |
|   511 | net IP validation distinguishes IPv4, IPv6, and invalid values           | `tools/run-node-tests.sh --stage 511` |
|   512 | path matchesGlob handles wildcard and path patterns                      | `tools/run-node-tests.sh --stage 512` |
|   513 | os constants expose frozen signal values                                 | `tools/run-node-tests.sh --stage 513` |
|   514 | zlib synchronous compression round trips and options                     | `tools/run-node-tests.sh --stage 514` |
|   515 | zlib constants and codes expose frozen status values                     | `tools/run-node-tests.sh --stage 515` |
|   516 | zlib asynchronous callbacks round trip and report errors                 | `tools/run-node-tests.sh --stage 516` |
|   517 | zlib crc32 computes strings, buffers, and seeds                          | `tools/run-node-tests.sh --stage 517` |
|   518 | zlib unzip detects deflate and gzip formats                              | `tools/run-node-tests.sh --stage 518` |
|   519 | StringDecoder preserves split UTF-8 sequences                            | `tools/run-node-tests.sh --stage 519` |
|   520 | tls surface exposes defaults and unsupported connect behavior            | `tools/run-node-tests.sh --stage 520` |
|   521 | tty streams expose non-interactive terminal surface                      | `tools/run-node-tests.sh --stage 521` |
|   522 | zlib transform streams compress and decompress data                      | `tools/run-node-tests.sh --stage 522` |
|   523 | zlib async iterable compression round trips data                         | `tools/run-node-tests.sh --stage 523` |
|   524 | util types identifies built-in collection and view types                 | `tools/run-node-tests.sh --stage 524` |
|   525 | stream promises finished tracks transform completion                     | `tools/run-node-tests.sh --stage 525` |
|   526 | stream web readable and writable reader/writer contracts                 | `tools/run-node-tests.sh --stage 526` |
|   527 | stream consumers read Web Streams as text JSON and buffers               | `tools/run-node-tests.sh --stage 527` |
|   528 | punycode converts Unicode domains and code points                        | `tools/run-node-tests.sh --stage 528` |
|   529 | module API exposes builtins and require helpers                          | `tools/run-node-tests.sh --stage 529` |
|   530 | diagnostics channels publish and unsubscribe subscribers                 | `tools/run-node-tests.sh --stage 530` |
|   531 | domain contexts run bind and dispose callbacks                           | `tools/run-node-tests.sh --stage 531` |
|   532 | readline promises question resolves and closes input                     | `tools/run-node-tests.sh --stage 532` |
|   533 | repl starts evaluates expressions and closes                             | `tools/run-node-tests.sh --stage 533` |
|   534 | constants exposes frozen filesystem and signal values                    | `tools/run-node-tests.sh --stage 534` |
|   535 | assert strict aliases assert and enforces strict comparisons             | `tools/run-node-tests.sh --stage 535` |
|   536 | sys aliases util and exposes formatting helpers                          | `tools/run-node-tests.sh --stage 536` |
|   537 | trace_events reports unknown builtin module errors                       | `tools/run-node-tests.sh --stage 537` |
|   538 | wasi reports unknown builtin module errors                               | `tools/run-node-tests.sh --stage 538` |
|   539 | inspector modules report unknown builtin module errors                   | `tools/run-node-tests.sh --stage 539` |
|   540 | util parseArgs handles booleans, strings, negation, and tokens           | `tools/run-node-tests.sh --stage 540` |
|   541 | util styleText applies ANSI styles and color options                     | `tools/run-node-tests.sh --stage 541` |
|   542 | util callbackify bridges promises and errors to callbacks                | `tools/run-node-tests.sh --stage 542` |
|   543 | util transferable abort helpers validate and preserve signals            | `tools/run-node-tests.sh --stage 543` |
|   544 | console module exposes Console and common output methods                 | `tools/run-node-tests.sh --stage 544` |
|   545 | URL static canParse validates inputs and parse returns URL objects       | `tools/run-node-tests.sh --stage 545` |
|   546 | v8 exposes heap statistics and unsupported coverage operations           | `tools/run-node-tests.sh --stage 546` |
|   547 | os helpers expose directories and user information                       | `tools/run-node-tests.sh --stage 547` |
|   548 | process metrics expose uptime memory and CPU usage                       | `tools/run-node-tests.sh --stage 548` |
|   549 | fs constants expose flags and are immutable                              | `tools/run-node-tests.sh --stage 549` |
|   550 | stream PassThrough forwards data and remains readable and writable       | `tools/run-node-tests.sh --stage 550` |
|   551 | process report exposes JavaScript reports and signal defaults            | `tools/run-node-tests.sh --stage 551` |
|   552 | fs promises glob asynchronously matches files from a cwd                 | `tools/run-node-tests.sh --stage 552` |
|   553 | dns servers, resolver, lookup callback, and promises are supported       | `tools/run-node-tests.sh --stage 553` |
|   554 | dgram UDP sockets bind, send packets, close, and expose unref            | `tools/run-node-tests.sh --stage 554` |
|   555 | https exposes TLS boundary methods and a global agent                    | `tools/run-node-tests.sh --stage 555` |
|   556 | http2 exposes constants and reports unsupported operations               | `tools/run-node-tests.sh --stage 556` |
|   557 | node:test reporters reports an unknown builtin module                    | `tools/run-node-tests.sh --stage 557` |
|   558 | sqlite builtin reports an unknown module boundary                        | `tools/run-node-tests.sh --stage 558` |
|   559 | cluster scheduling constants and primary-role flags are exposed          | `tools/run-node-tests.sh --stage 559` |
|   560 | cluster workers report connected state across fork lifecycle             | `tools/run-node-tests.sh --stage 560` |
|   561 | cluster workers expose isDead and destroy lifecycle methods              | `tools/run-node-tests.sh --stage 561` |
|   562 | cluster schedulingPolicy defaults to round-robin scheduling              | `tools/run-node-tests.sh --stage 562` |
|   563 | cluster setupPrimary cumulatively updates execution settings             | `tools/run-node-tests.sh --stage 563` |
|   564 | cluster setupPrimary defaults mirror process execution settings          | `tools/run-node-tests.sh --stage 564` |
|   565 | cluster legacy setupMaster and isMaster aliases remain compatible        | `tools/run-node-tests.sh --stage 565` |
|   566 | cluster workers exposes a primary-process worker registry                | `tools/run-node-tests.sh --stage 566` |
|   567 | cluster workers registry removes entries after worker exit               | `tools/run-node-tests.sh --stage 567` |
|   568 | cluster worker processes expose IPC connection methods                   | `tools/run-node-tests.sh --stage 568` |
|   569 | child_process exec APIs expose callbacks and synchronous variants        | `tools/run-node-tests.sh --stage 569` |
|   570 | child_process spawn exposes streams and process metadata                 | `tools/run-node-tests.sh --stage 570` |
|   571 | child_process emits spawn, exit, and close lifecycle events              | `tools/run-node-tests.sh --stage 571` |
|   572 | child_process reports spawn failures with enriched ENOENT errors         | `tools/run-node-tests.sh --stage 572` |
|   573 | child_process exec and execFile report command failure metadata          | `tools/run-node-tests.sh --stage 573` |
|   574 | child_process spawnSync reports synchronous launch errors                | `tools/run-node-tests.sh --stage 574` |
|   575 | child_process synchronous exec APIs return configured output             | `tools/run-node-tests.sh --stage 575` |
|   576 | child_process kill reports signal termination and killed state           | `tools/run-node-tests.sh --stage 576` |
|   577 | child_process exposes chainable ref and unref lifecycle methods          | `tools/run-node-tests.sh --stage 577` |
|   578 | child_process exposes the internal fork-child entry point                | `tools/run-node-tests.sh --stage 578` |
|   579 | child_process stdio streams expose event and encoding methods            | `tools/run-node-tests.sh --stage 579` |
|   580 | child_process stdout emits data and end events for environment output    | `tools/run-node-tests.sh --stage 580` |
|   581 | child_process ref and unref preserve legacy return contracts             | `tools/run-node-tests.sh --stage 581` |
|   582 | child_process exec supports Buffer output encoding                       | `tools/run-node-tests.sh --stage 582` |
|   583 | child_process inherits environment variables for spawned commands        | `tools/run-node-tests.sh --stage 583` |
|   584 | child_process emits spawn before exit and stream close events            | `tools/run-node-tests.sh --stage 584` |
|   585 | child_process stdio streams expose readable and writable state           | `tools/run-node-tests.sh --stage 585` |
|   586 | process.send validates callbacks and unsupported handle arguments        | `tools/run-node-tests.sh --stage 586` |
|   587 | child_process fork send validates unsupported callback arguments         | `tools/run-node-tests.sh --stage 587` |
|   588 | child_process fork reports normal child exit status                      | `tools/run-node-tests.sh --stage 588` |
|   589 | child_process exposes destroy and Symbol.dispose termination             | `tools/run-node-tests.sh --stage 589` |
|   590 | process exposes active-resource information as an array                  | `tools/run-node-tests.sh --stage 590` |
|   591 | process exposes available memory as a non-negative number                | `tools/run-node-tests.sh --stage 591` |
|   592 | process hrtime.bigint returns monotonic bigint timestamps                | `tools/run-node-tests.sh --stage 592` |
|   593 | process source-map toggling is callable and returns undefined            | `tools/run-node-tests.sh --stage 593` |
|   594 | process emitWarning is callable and returns undefined                    | `tools/run-node-tests.sh --stage 594` |
|   595 | process debugPort is numeric and writable                                | `tools/run-node-tests.sh --stage 595` |
|   596 | process sourceMapsEnabled exposes a boolean default                      | `tools/run-node-tests.sh --stage 596` |
|   597 | process release exposes Node runtime metadata                            | `tools/run-node-tests.sh --stage 597` |
|   598 | process allowedNodeEnvironmentFlags exposes a Set contract               | `tools/run-node-tests.sh --stage 598` |
|   599 | process execArgv exposes an empty runtime-argument array                 | `tools/run-node-tests.sh --stage 599` |
|   600 | process argv0 identifies the Node-compatible runtime                     | `tools/run-node-tests.sh --stage 600` |
|   601 | process features exposes a runtime capability object                     | `tools/run-node-tests.sh --stage 601` |
|   602 | process deprecation flags default to false booleans                      | `tools/run-node-tests.sh --stage 602` |
|   603 | process version exposes a Node-style semantic version string             | `tools/run-node-tests.sh --stage 603` |
|   604 | process versions exposes the Node version entry                          | `tools/run-node-tests.sh --stage 604` |
|   605 | process versions exposes a semantic V8 version entry                     | `tools/run-node-tests.sh --stage 605` |
|   606 | process versions exposes a semantic libuv version entry                  | `tools/run-node-tests.sh --stage 606` |
|   607 | process versions exposes a semantic OpenSSL version entry                | `tools/run-node-tests.sh --stage 607` |
|   608 | process versions exposes a semantic zlib version entry                   | `tools/run-node-tests.sh --stage 608` |
|   609 | process versions exposes a numeric native modules ABI entry              | `tools/run-node-tests.sh --stage 609` |
|   610 | process versions exposes a numeric N-API version entry                   | `tools/run-node-tests.sh --stage 610` |
|   611 | process versions exposes a semantic Acorn version entry                  | `tools/run-node-tests.sh --stage 611` |
|   612 | process versions exposes a semantic Ada version entry                    | `tools/run-node-tests.sh --stage 612` |
|   613 | process versions exposes a timezone database version entry               | `tools/run-node-tests.sh --stage 613` |
|   614 | process versions exposes a semantic Brotli version entry                 | `tools/run-node-tests.sh --stage 614` |
|   615 | process versions exposes a semantic nbytes version entry                 | `tools/run-node-tests.sh --stage 615` |
|   616 | process versions exposes a locale-data CLDR version entry                | `tools/run-node-tests.sh --stage 616` |
|   617 | process versions exposes a semantic ICU version entry                    | `tools/run-node-tests.sh --stage 617` |
|   618 | process versions exposes a semantic nghttp2 version entry                | `tools/run-node-tests.sh --stage 618` |
|   619 | process versions exposes a semantic llhttp version entry                 | `tools/run-node-tests.sh --stage 619` |
|   620 | process versions exposes a semantic nghttp3 version entry                | `tools/run-node-tests.sh --stage 620` |
|   621 | process versions exposes a semantic ngtcp2 version entry                 | `tools/run-node-tests.sh --stage 621` |
|   622 | process versions exposes a semantic simdutf version entry                | `tools/run-node-tests.sh --stage 622` |
|   623 | process versions exposes a Unicode data version entry                    | `tools/run-node-tests.sh --stage 623` |
|   624 | process versions exposes a semantic Undici version entry                 | `tools/run-node-tests.sh --stage 624` |
|   625 | process versions exposes a semantic CJS lexer version entry              | `tools/run-node-tests.sh --stage 625` |
|   626 | process title identifies the Node-compatible runtime                     | `tools/run-node-tests.sh --stage 626` |
|   627 | process getBuiltinModule resolves builtins and rejects unknown names     | `tools/run-node-tests.sh --stage 627` |
|   628 | process loadEnvFile exposes a callable harness-safe surface              | `tools/run-node-tests.sh --stage 628` |
|   629 | process finalization exposes registration method contracts               | `tools/run-node-tests.sh --stage 629` |
|   630 | process permission exposes conservative capability checks                | `tools/run-node-tests.sh --stage 630` |
|   631 | process resourceUsage exposes deterministic numeric metrics              | `tools/run-node-tests.sh --stage 631` |
|   632 | process cpuUsage exposes deterministic user and system metrics           | `tools/run-node-tests.sh --stage 632` |
|   633 | process cpuUsage accepts a previous sample for delta metrics             | `tools/run-node-tests.sh --stage 633` |
|   634 | process memoryUsage exposes all numeric memory metrics                   | `tools/run-node-tests.sh --stage 634` |
|   635 | process memoryUsage.rss exposes a numeric resident-set metric            | `tools/run-node-tests.sh --stage 635` |
|   636 | process uptime exposes a non-negative monotonic seconds value            | `tools/run-node-tests.sh --stage 636` |
|   637 | process nextTick forwards arguments after synchronous scheduling         | `tools/run-node-tests.sh --stage 637` |
|   638 | process exitCode is writable without terminating the harness             | `tools/run-node-tests.sh --stage 638` |
|   639 | process once delivers one event and removes its listener                 | `tools/run-node-tests.sh --stage 639` |
|   640 | process removeAllListeners clears only the selected event                | `tools/run-node-tests.sh --stage 640` |
|   641 | process removeListener removes only the targeted callback                | `tools/run-node-tests.sh --stage 641` |
|   642 | process emitWarning preserves warning name, message, and code            | `tools/run-node-tests.sh --stage 642` |
|   643 | process emitWarning normalizes Error input to warning metadata           | `tools/run-node-tests.sh --stage 643` |
|   644 | process emit fans out arguments and reports listener presence            | `tools/run-node-tests.sh --stage 644` |
|   645 | process on and once return the process object for chaining               | `tools/run-node-tests.sh --stage 645` |
|   646 | process argv exposes a non-empty array of string arguments               | `tools/run-node-tests.sh --stage 646` |
|   647 | process env coerces assignments to strings and supports deletion         | `tools/run-node-tests.sh --stage 647` |
|   648 | process platform and arch expose non-empty runtime identity strings      | `tools/run-node-tests.sh --stage 648` |
|   649 | process config exposes a variables object                                | `tools/run-node-tests.sh --stage 649` |
|   650 | process release exposes string metadata fields                           | `tools/run-node-tests.sh --stage 650` |
|   651 | process allowed environment flags exposes a set-like collection          | `tools/run-node-tests.sh --stage 651` |
|   652 | process features exposes a boolean inspector capability                  | `tools/run-node-tests.sh --stage 652` |
|   653 | process deprecation policy flags expose boolean defaults                 | `tools/run-node-tests.sh --stage 653` |
|   654 | process debugPort exposes a finite non-negative number                   | `tools/run-node-tests.sh --stage 654` |
|   655 | process getActiveResourcesInfo returns resource-name strings             | `tools/run-node-tests.sh --stage 655` |
|   656 | process availableMemory returns a finite non-negative number             | `tools/run-node-tests.sh --stage 656` |
|   657 | process source-map controls expose callable and boolean contracts        | `tools/run-node-tests.sh --stage 657` |
|   658 | process title exposes a writable string value                            | `tools/run-node-tests.sh --stage 658` |
|   659 | process getBuiltinModule resolves built-ins with and without node prefix | `tools/run-node-tests.sh --stage 659` |
|   660 | process permission has exposes a boolean capability result               | `tools/run-node-tests.sh --stage 660` |
|   661 | process resourceUsage exposes finite non-negative numeric metrics        | `tools/run-node-tests.sh --stage 661` |
|   662 | process cpuUsage exposes finite non-negative user and system metrics     | `tools/run-node-tests.sh --stage 662` |
|   663 | process cpuUsage accepts a previous sample for delta metrics             | `tools/run-node-tests.sh --stage 663` |
|   664 | process memoryUsage exposes finite non-negative numeric metrics          | `tools/run-node-tests.sh --stage 664` |
|   665 | process memoryUsage.rss exposes a finite non-negative metric             | `tools/run-node-tests.sh --stage 665` |
|   666 | process uptime exposes a finite non-negative monotonic seconds value     | `tools/run-node-tests.sh --stage 666` |
|   667 | process nextTick forwards callback arguments through the scheduler       | `tools/run-node-tests.sh --stage 667` |
|   668 | process exitCode is writable without terminating the harness             | `tools/run-node-tests.sh --stage 668` |
|   669 | process once delivers one event and removes its listener                 | `tools/run-node-tests.sh --stage 669` |
|   670 | process removeAllListeners clears only the selected event                | `tools/run-node-tests.sh --stage 670` |
|   671 | process removeListener removes only the targeted callback                | `tools/run-node-tests.sh --stage 671` |
|   672 | process emit fans out arguments and reports listener presence            | `tools/run-node-tests.sh --stage 672` |
|   673 | process on and once return the process object for chaining               | `tools/run-node-tests.sh --stage 673` |
|   674 | process argv0 exposes a non-empty launch identity string                 | `tools/run-node-tests.sh --stage 674` |
|   675 | process execArgv exposes an array of strings                             | `tools/run-node-tests.sh --stage 675` |
|   676 | process versions exposes a non-empty Node version string                 | `tools/run-node-tests.sh --stage 676` |
|   677 | process versions entries expose non-empty strings                        | `tools/run-node-tests.sh --stage 677` |
|   678 | process execPath exposes a non-empty executable path string              | `tools/run-node-tests.sh --stage 678` |
|   679 | process hrtime exposes a two-component high-resolution tuple             | `tools/run-node-tests.sh --stage 679` |
|   680 | process hrtime.bigint exposes a non-negative nanosecond BigInt           | `tools/run-node-tests.sh --stage 680` |
|   681 | process umask exposes a numeric mask and reversible read behavior        | `tools/run-node-tests.sh --stage 681` |
|   682 | process pid exposes a positive integer process identifier                | `tools/run-node-tests.sh --stage 682` |
|   683 | process ppid exposes a non-negative integer parent identifier            | `tools/run-node-tests.sh --stage 683` |
|   684 | process send exposes a boolean no-channel result                         | `tools/run-node-tests.sh --stage 684` |
|   685 | process cwd exposes a non-empty current working directory string         | `tools/run-node-tests.sh --stage 685` |
|   686 | process chdir accepts the current directory and preserves cwd            | `tools/run-node-tests.sh --stage 686` |
|   687 | process stdout exposes a writable stream-like object                     | `tools/run-node-tests.sh --stage 687` |
|   688 | process stderr exposes a writable stream-like object                     | `tools/run-node-tests.sh --stage 688` |
|   689 | process stdin exposes a stream-like listener interface                   | `tools/run-node-tests.sh --stage 689` |
|   690 | process stdin on returns the stream for chaining                         | `tools/run-node-tests.sh --stage 690` |
|   691 | process stdout write reports a successful empty write                    | `tools/run-node-tests.sh --stage 691` |
|   692 | process stderr write reports a successful empty write                    | `tools/run-node-tests.sh --stage 692` |
|   693 | process noDeprecation is a writable boolean policy flag                  | `tools/run-node-tests.sh --stage 693` |
|   694 | process traceDeprecation is a writable boolean policy flag               | `tools/run-node-tests.sh --stage 694` |
|   695 | process throwDeprecation is a writable boolean policy flag               | `tools/run-node-tests.sh --stage 695` |
|   696 | process stdin pause and resume are callable and chainable                | `tools/run-node-tests.sh --stage 696` |
|   697 | process stdout on returns the stream for chaining                        | `tools/run-node-tests.sh --stage 697` |
|   698 | process stderr on returns the stream for chaining                        | `tools/run-node-tests.sh --stage 698` |
|   699 | process stdin setEncoding returns the stream for chaining                | `tools/run-node-tests.sh --stage 699` |
|   700 | process stdout setEncoding returns the stream for chaining               | `tools/run-node-tests.sh --stage 700` |
|   701 | process stderr setEncoding returns the stream for chaining               | `tools/run-node-tests.sh --stage 701` |
|   702 | process stdout end returns the stream for chaining                       | `tools/run-node-tests.sh --stage 702` |
|   703 | process stderr end returns the stream for chaining                       | `tools/run-node-tests.sh --stage 703` |
|   704 | process stdout cork and uncork are callable and chainable                | `tools/run-node-tests.sh --stage 704` |
|   705 | process stderr cork and uncork are callable and chainable                | `tools/run-node-tests.sh --stage 705` |
|   706 | process stdout fd exposes a non-negative integer descriptor              | `tools/run-node-tests.sh --stage 706` |
|   707 | process stderr fd exposes a non-negative integer descriptor              | `tools/run-node-tests.sh --stage 707` |
|   708 | process stdout once returns the stream for chaining                      | `tools/run-node-tests.sh --stage 708` |
|   709 | process stderr once returns the stream for chaining                      | `tools/run-node-tests.sh --stage 709` |
|   710 | process stdout removeListener returns the stream for chaining            | `tools/run-node-tests.sh --stage 710` |
|   711 | process stderr removeListener returns the stream for chaining            | `tools/run-node-tests.sh --stage 711` |
|   712 | process stdout addListener returns the stream for chaining               | `tools/run-node-tests.sh --stage 712` |
|   713 | process stderr addListener returns the stream for chaining               | `tools/run-node-tests.sh --stage 713` |
|   714 | process stdout listenerCount reports the lightweight listener state      | `tools/run-node-tests.sh --stage 714` |
|   715 | process stderr listenerCount reports the lightweight listener state      | `tools/run-node-tests.sh --stage 715` |
|   716 | process stdout eventNames reports the lightweight listener state         | `tools/run-node-tests.sh --stage 716` |
|   717 | process stderr eventNames reports the lightweight listener state         | `tools/run-node-tests.sh --stage 717` |
|   718 | process stdout getMaxListeners reports the Node default limit            | `tools/run-node-tests.sh --stage 718` |
|   719 | process stderr getMaxListeners reports the Node default limit            | `tools/run-node-tests.sh --stage 719` |
|   720 | process stdout setMaxListeners updates the listener limit                | `tools/run-node-tests.sh --stage 720` |
|   721 | process stderr setMaxListeners updates the listener limit                | `tools/run-node-tests.sh --stage 721` |
|   722 | process stdout rawListeners reports the lightweight listener state       | `tools/run-node-tests.sh --stage 722` |
|   723 | process stderr rawListeners reports the lightweight listener state       | `tools/run-node-tests.sh --stage 723` |
|   724 | process stdout prependListener returns the stream for chaining           | `tools/run-node-tests.sh --stage 724` |
|   725 | process stderr prependListener returns the stream for chaining           | `tools/run-node-tests.sh --stage 725` |
|   726 | process stdout prependOnceListener returns the stream for chaining       | `tools/run-node-tests.sh --stage 726` |
|   727 | process stderr prependOnceListener returns the stream for chaining       | `tools/run-node-tests.sh --stage 727` |
|   728 | process stdout off returns the stream for chaining                       | `tools/run-node-tests.sh --stage 728` |
|   729 | process stderr off returns the stream for chaining                       | `tools/run-node-tests.sh --stage 729` |
|   730 | process stdout emit reports no unhandled listeners                       | `tools/run-node-tests.sh --stage 730` |
|   731 | process stderr emit reports no unhandled listeners                       | `tools/run-node-tests.sh --stage 731` |
|   732 | process stdout listeners reports the lightweight listener state          | `tools/run-node-tests.sh --stage 732` |
|   733 | process stderr listeners reports the lightweight listener state          | `tools/run-node-tests.sh --stage 733` |
|   734 | process stdout async iterator is empty and awaitable                     | `tools/run-node-tests.sh --stage 734` |
|   735 | process stderr async iterator is empty and awaitable                     | `tools/run-node-tests.sh --stage 735` |
|   736 | process stdout destroy is non-destructive and chainable                  | `tools/run-node-tests.sh --stage 736` |
|   737 | process stderr destroy is non-destructive and chainable                  | `tools/run-node-tests.sh --stage 737` |
|   738 | process stdout writable state reports an active stream                   | `tools/run-node-tests.sh --stage 738` |
|   739 | process stderr writable state reports an active stream                   | `tools/run-node-tests.sh --stage 739` |
|   740 | process stdout writableNeedDrain reports no pending drain                | `tools/run-node-tests.sh --stage 740` |
|   741 | process stderr writableNeedDrain reports no pending drain                | `tools/run-node-tests.sh --stage 741` |
|   742 | process stdout writableHighWaterMark is positive                         | `tools/run-node-tests.sh --stage 742` |
|   743 | process stderr writableHighWaterMark is positive                         | `tools/run-node-tests.sh --stage 743` |
|   744 | process stdout readable state matches a process output stream            | `tools/run-node-tests.sh --stage 744` |
|   745 | process stderr readable state matches a process output stream            | `tools/run-node-tests.sh --stage 745` |
|   746 | process stdout readableHighWaterMark matches local Node                  | `tools/run-node-tests.sh --stage 746` |
|   747 | process stderr readableHighWaterMark matches local Node                  | `tools/run-node-tests.sh --stage 747` |
|   748 | process stdout readableLength starts empty                               | `tools/run-node-tests.sh --stage 748` |
|   749 | process stderr readableLength starts empty                               | `tools/run-node-tests.sh --stage 749` |
|   750 | process stdout bytesWritten starts at zero                               | `tools/run-node-tests.sh --stage 750` |
|   751 | process stderr bytesWritten starts at zero                               | `tools/run-node-tests.sh --stage 751` |
|   752 | process stdout writableCorked starts at zero                             | `tools/run-node-tests.sh --stage 752` |
|   753 | process stderr writableCorked starts at zero                             | `tools/run-node-tests.sh --stage 753` |
|   754 | process stdout pending reports no pending operation                      | `tools/run-node-tests.sh --stage 754` |
|   755 | process stderr pending reports no pending operation                      | `tools/run-node-tests.sh --stage 755` |
|   756 | process stdout writableObjectMode is disabled                            | `tools/run-node-tests.sh --stage 756` |
|   757 | process stderr writableObjectMode is disabled                            | `tools/run-node-tests.sh --stage 757` |
|   758 | process stdout readableObjectMode is disabled                            | `tools/run-node-tests.sh --stage 758` |
|   759 | process stderr readableObjectMode is disabled                            | `tools/run-node-tests.sh --stage 759` |
|   760 | process stdout stdio methods are present and chainable                   | `tools/run-node-tests.sh --stage 760` |
|   761 | process stderr stdio methods are present and chainable                   | `tools/run-node-tests.sh --stage 761` |
|   762 | process stdin readable state matches an active input stream              | `tools/run-node-tests.sh --stage 762` |
|   763 | process stdin readableFlowing starts unset                               | `tools/run-node-tests.sh --stage 763` |
|   764 | process stdin readableHighWaterMark matches local Node                   | `tools/run-node-tests.sh --stage 764` |
|   765 | process stdin readableLength starts empty                                | `tools/run-node-tests.sh --stage 765` |
|   766 | process stdin readableObjectMode is disabled                             | `tools/run-node-tests.sh --stage 766` |
|   767 | process stdin readable methods expose empty-input behavior               | `tools/run-node-tests.sh --stage 767` |
|   768 | process stdin isPaused reports an active input flow                      | `tools/run-node-tests.sh --stage 768` |
|   769 | process stdin exposes stdio lifecycle methods                            | `tools/run-node-tests.sh --stage 769` |
|   770 | process stdin exposes local stream state                                 | `tools/run-node-tests.sh --stage 770` |
|   771 | process stdin exposes stream lifecycle state                             | `tools/run-node-tests.sh --stage 771` |
|   772 | process stdin exposes readable stream methods                            | `tools/run-node-tests.sh --stage 772` |
|   773 | process stdin exposes close and pending state                            | `tools/run-node-tests.sh --stage 773` |
|   774 | process stdin exposes async disposal                                     | `tools/run-node-tests.sh --stage 774` |
|   775 | process stdin exposes ReadStream type metadata                           | `tools/run-node-tests.sh --stage 775` |
|   776 | process stdin aligns ReadStream range metadata                           | `tools/run-node-tests.sh --stage 776` |
|   777 | process stdin async disposal returns a promise                           | `tools/run-node-tests.sh --stage 777` |
|   778 | process stdout aligns Socket type and buffer size                        | `tools/run-node-tests.sh --stage 778` |
|   779 | process stderr aligns Socket type and buffer size                        | `tools/run-node-tests.sh --stage 779` |
|   780 | process stdio exposes async disposal                                     | `tools/run-node-tests.sh --stage 780` |
|   781 | process stdio async disposal returns promises                            | `tools/run-node-tests.sh --stage 781` |
|   782 | process exposes identity and umask methods                               | `tools/run-node-tests.sh --stage 782` |
|   783 | process exposes credential helper methods                                | `tools/run-node-tests.sh --stage 783` |
|   784 | process exposes uncaught exception capture state                         | `tools/run-node-tests.sh --stage 784` |
|   785 | process exposes warning emission                                         | `tools/run-node-tests.sh --stage 785` |
|   786 | process exposes runtime resource methods                                 | `tools/run-node-tests.sh --stage 786` |
|   787 | process exposes active handle and request inspection                     | `tools/run-node-tests.sh --stage 787` |
|   788 | process exposes control method surface                                   | `tools/run-node-tests.sh --stage 788` |
|   789 | process exposes low-level binding methods                                | `tools/run-node-tests.sh --stage 789` |
|   790 | process exposes debug and scheduling helper methods                      | `tools/run-node-tests.sh --stage 790` |
|   791 | process exposes ref and unref helpers                                    | `tools/run-node-tests.sh --stage 791` |
|   792 | process features expose Node capability flags                            | `tools/run-node-tests.sh --stage 792` |
|   793 | process exposes configuration metadata                                   | `tools/run-node-tests.sh --stage 793` |
|   794 | process exposes diagnostic report object                                 | `tools/run-node-tests.sh --stage 794` |
|   795 | process exposes finalization lifecycle methods                           | `tools/run-node-tests.sh --stage 795` |
|   796 | process exposes permission query helper                                  | `tools/run-node-tests.sh --stage 796` |
|   797 | process exposes release metadata                                         | `tools/run-node-tests.sh --stage 797` |
|   798 | process exposes builtin module loading                                   | `tools/run-node-tests.sh --stage 798` |
|   799 | process exposes allowed environment flags                                | `tools/run-node-tests.sh --stage 799` |
|   800 | process exposes launch metadata                                          | `tools/run-node-tests.sh --stage 800` |
|   801 | process exposes resource metric shapes                                   | `tools/run-node-tests.sh --stage 801` |
|   802 | process exposes high-resolution bigint timing                            | `tools/run-node-tests.sh --stage 802` |
|   803 | module exposes core API helpers                                          | `tools/run-node-tests.sh --stage 803` |
|   804 | module exposes modern helper methods                                     | `tools/run-node-tests.sh --stage 804` |
|   805 | module exposes metadata and constructors                                 | `tools/run-node-tests.sh --stage 805` |
|   806 | module createRequire resolves builtins                                   | `tools/run-node-tests.sh --stage 806` |
|   807 | module exposes loader hook methods                                       | `tools/run-node-tests.sh --stage 807` |
|   808 | module detects standard builtin names                                    | `tools/run-node-tests.sh --stage 808` |
|   809 | module exposes static Module helpers                                     | `tools/run-node-tests.sh --stage 809` |
|   810 | module exposes static loader state                                       | `tools/run-node-tests.sh --stage 810` |
|   811 | module exposes standard extension handlers                               | `tools/run-node-tests.sh --stage 811` |
|   812 | module exposes static path helpers                                       | `tools/run-node-tests.sh --stage 812` |
|   813 | module exposes static resolution helpers                                 | `tools/run-node-tests.sh --stage 813` |
|   814 | assert exposes core assertion API                                        | `tools/run-node-tests.sh --stage 814` |
|   815 | buffer exposes core static API                                           | `tools/run-node-tests.sh --stage 815` |
|   816 | buffer exposes modern encoding helpers                                   | `tools/run-node-tests.sh --stage 816` |
|   817 | events exposes core emitter API                                          | `tools/run-node-tests.sh --stage 817` |
|   818 | events inspects listeners and max-listener limits                        | `tools/run-node-tests.sh --stage 818` |
|   819 | stream exposes core constructors and helpers                             | `tools/run-node-tests.sh --stage 819` |
|   820 | stream promises exposes pipeline and finished                            | `tools/run-node-tests.sh --stage 820` |
|   821 | util exposes core formatting and helper API                              | `tools/run-node-tests.sh --stage 821` |
|   822 | os exposes core platform and resource API                                | `tools/run-node-tests.sh --stage 822` |
|   823 | os exposes modern parallelism and priority API                           | `tools/run-node-tests.sh --stage 823` |
|   824 | path exposes core parsing and joining API                                | `tools/run-node-tests.sh --stage 824` |
|   825 | path exposes glob matching helper                                        | `tools/run-node-tests.sh --stage 825` |
|   826 | url exposes core URL and parsing API                                     | `tools/run-node-tests.sh --stage 826` |
|   827 | timers exposes callback and promise APIs                                 | `tools/run-node-tests.sh --stage 827` |
|   828 | console exposes core logging and constructor API                         | `tools/run-node-tests.sh --stage 828` |
|   829 | tty exposes detection and stream constructors                            | `tools/run-node-tests.sh --stage 829` |
|   830 | querystring exposes parse and stringify API                              | `tools/run-node-tests.sh --stage 830` |
|   831 | string decoder exposes incremental decoding API                          | `tools/run-node-tests.sh --stage 831` |
|   832 | diagnostics channel exposes publish and subscribe API                    | `tools/run-node-tests.sh --stage 832` |
|   833 | perf hooks exposes timing and observer API                               | `tools/run-node-tests.sh --stage 833` |
|   834 | worker threads exposes core messaging API                                | `tools/run-node-tests.sh --stage 834` |
|   835 | crypto exposes core hashing and randomness API                           | `tools/run-node-tests.sh --stage 835` |
|   836 | zlib exposes compression and stream API                                  | `tools/run-node-tests.sh --stage 836` |
|   837 | dns exposes callback and promise resolver API                            | `tools/run-node-tests.sh --stage 837` |
|   838 | http exposes client and server core API                                  | `tools/run-node-tests.sh --stage 838` |
|   839 | https exposes secure client and server API                               | `tools/run-node-tests.sh --stage 839` |
|   840 | net exposes TCP client and server core API                               | `tools/run-node-tests.sh --stage 840` |
|   841 | dgram exposes UDP socket API                                             | `tools/run-node-tests.sh --stage 841` |
|   842 | tls exposes secure transport API                                         | `tools/run-node-tests.sh --stage 842` |
|   843 | child process exposes process creation API                               | `tools/run-node-tests.sh --stage 843` |
|   844 | v8 exposes serialization and heap inspection API                         | `tools/run-node-tests.sh --stage 844` |
|   845 | vm exposes contexts scripts and module API                               | `tools/run-node-tests.sh --stage 845` |
|   846 | readline exposes interface and terminal API                              | `tools/run-node-tests.sh --stage 846` |
|   847 | repl exposes interactive evaluation API                                  | `tools/run-node-tests.sh --stage 847` |
|   848 | cluster exposes primary and worker API                                   | `tools/run-node-tests.sh --stage 848` |
|   849 | trace events exposes category tracing API                                | `tools/run-node-tests.sh --stage 849` |
|   850 | wasi exposes WebAssembly system interface API                            | `tools/run-node-tests.sh --stage 850` |
|   851 | async hooks exposes execution context API                                | `tools/run-node-tests.sh --stage 851` |
|   852 | constants exposes system and module constant groups                      | `tools/run-node-tests.sh --stage 852` |
|   853 | punycode exposes Unicode domain conversion API                           | `tools/run-node-tests.sh --stage 853` |
|   854 | domain exposes legacy error context API                                  | `tools/run-node-tests.sh --stage 854` |
|   855 | inspector exposes debugging session API                                  | `tools/run-node-tests.sh --stage 855` |
|   856 | test exposes test runner and lifecycle API                               | `tools/run-node-tests.sh --stage 856` |
|   857 | process exposes modern memory and resource API                           | `tools/run-node-tests.sh --stage 857` |
|   858 | util types exposes specialized type predicates                           | `tools/run-node-tests.sh --stage 858` |
|   859 | sqlite exposes synchronous database API                                  | `tools/run-node-tests.sh --stage 859` |
|   860 | http2 exposes HTTP/2 client and server API                               | `tools/run-node-tests.sh --stage 860` |
|   861 | sys exposes legacy formatting and type API                               | `tools/run-node-tests.sh --stage 861` |
|   862 | test reporters exposes standard reporter factories                       | `tools/run-node-tests.sh --stage 862` |
|   863 | inspector promises exposes async debugging API                           | `tools/run-node-tests.sh --stage 863` |
|   864 | process report exposes diagnostic report API                             | `tools/run-node-tests.sh --stage 864` |
|   865 | stream web exposes Web Streams constructors and helpers                  | `tools/run-node-tests.sh --stage 865` |
|   866 | stream consumers exposes conversion helpers                              | `tools/run-node-tests.sh --stage 866` |
|   867 | assert strict exposes strict assertion API                               | `tools/run-node-tests.sh --stage 867` |
|   868 | fs promises exposes promise filesystem API                               | `tools/run-node-tests.sh --stage 868` |
|   869 | worker threads exposes environment and transfer helpers                  | `tools/run-node-tests.sh --stage 869` |
|   870 | fs exposes callback and promise glob API                                 | `tools/run-node-tests.sh --stage 870` |
|   871 | crypto exposes Web Crypto compatibility API                              | `tools/run-node-tests.sh --stage 871` |
|   872 | timers promises exposes complete promise timer API                       | `tools/run-node-tests.sh --stage 872` |
|   873 | util exposes modern parsing and abort helpers                            | `tools/run-node-tests.sh --stage 873` |
|   874 | url exposes URLPattern matching API                                      | `tools/run-node-tests.sh --stage 874` |
|   875 | fs exposes callback promise and sync copy API                            | `tools/run-node-tests.sh --stage 875` |
|   876 | fs exposes file watching API                                             | `tools/run-node-tests.sh --stage 876` |
|   877 | fs exposes directory and stream constructors                             | `tools/run-node-tests.sh --stage 877` |
|   878 | crypto exposes key and certificate constructors                          | `tools/run-node-tests.sh --stage 878` |
|   879 | crypto exposes signing and key generation API                            | `tools/run-node-tests.sh --stage 879` |
|   880 | crypto exposes symmetric and derivation API                              | `tools/run-node-tests.sh --stage 880` |
|   881 | os exposes platform and user environment API                             | `tools/run-node-tests.sh --stage 881` |
|   882 | process exposes builtin loading helpers                                  | `tools/run-node-tests.sh --stage 882` |
|   883 | stream exposes Web Stream adapter helpers                                | `tools/run-node-tests.sh --stage 883` |
|   884 | stream promises exposes pipeline helpers                                 | `tools/run-node-tests.sh --stage 884` |
|   885 | net exposes BlockList rule API                                           | `tools/run-node-tests.sh --stage 885` |
|   886 | http2 exposes settings conversion API                                    | `tools/run-node-tests.sh --stage 886` |
|   887 | zlib exposes raw Brotli and unzip algorithms                             | `tools/run-node-tests.sh --stage 887` |
|   888 | crypto exposes usable hash chaining contract                             | `tools/run-node-tests.sh --stage 888` |
|   889 | crypto exposes usable HMAC chaining contract                             | `tools/run-node-tests.sh --stage 889` |
|   890 | crypto exposes timing-safe equality comparison                           | `tools/run-node-tests.sh --stage 890` |
|   891 | crypto exposes algorithm inventory                                       | `tools/run-node-tests.sh --stage 891` |
|   892 | crypto Web Crypto exposes digest                                         | `tools/run-node-tests.sh --stage 892` |
|   893 | crypto exposes synchronous random fill                                   | `tools/run-node-tests.sh --stage 893` |
|   894 | crypto exposes bounded random integer                                    | `tools/run-node-tests.sh --stage 894` |
|   895 | crypto exposes version 4 random UUIDs                                    | `tools/run-node-tests.sh --stage 895` |
|   896 | crypto exposes buffer random bytes                                       | `tools/run-node-tests.sh --stage 896` |
|   897 | crypto Web Crypto fills random typed arrays                              | `tools/run-node-tests.sh --stage 897` |
|   898 | crypto hash exposes canonical SHA-256 digest                             | `tools/run-node-tests.sh --stage 898` |
|   899 | crypto HMAC exposes canonical SHA-256 digest                             | `tools/run-node-tests.sh --stage 899` |
|   900 | crypto exposes usable secret key objects                                 | `tools/run-node-tests.sh --stage 900` |
|   901 | crypto exposes synchronous HKDF derivation                               | `tools/run-node-tests.sh --stage 901` |
|   902 | crypto exposes synchronous PBKDF2 derivation                             | `tools/run-node-tests.sh --stage 902` |
|   903 | crypto exposes FIPS mode state controls                                  | `tools/run-node-tests.sh --stage 903` |
|   904 | crypto exposes asynchronous PBKDF2 derivation                            | `tools/run-node-tests.sh --stage 904` |
|   905 | crypto exposes asynchronous HKDF derivation                              | `tools/run-node-tests.sh --stage 905` |
|   906 | crypto hash exposes default Buffer digest                                | `tools/run-node-tests.sh --stage 906` |
|   907 | process exposes builtin module lookup                                    | `tools/run-node-tests.sh --stage 907` |
|   908 | module exposes complete builtin inventory                                | `tools/run-node-tests.sh --stage 908` |
|   909 | module exposes builtin predicate normalization                           | `tools/run-node-tests.sh --stage 909` |
|   910 | process validates thread CPU usage arguments                             | `tools/run-node-tests.sh --stage 910` |
|   911 | process validates and tracks umask values                                | `tools/run-node-tests.sh --stage 911` |
|   912 | process validates UID and GID setter arguments                           | `tools/run-node-tests.sh --stage 912` |
|   913 | process validates next tick callback arguments                           | `tools/run-node-tests.sh --stage 913` |
|   914 | process exposes monotonic uptime                                         | `tools/run-node-tests.sh --stage 914` |
|   915 | harness injects per-script filename and dirname                          | `tools/run-node-tests.sh --stage 915` |
|   916 | module exposes absolute-path createRequire                               | `tools/run-node-tests.sh --stage 916` |
|   917 | process exposes release metadata                                         | `tools/run-node-tests.sh --stage 917` |
|   918 | process exposes parent PID metadata                                      | `tools/run-node-tests.sh --stage 918` |
|   919 | process exposes environment round trips                                  | `tools/run-node-tests.sh --stage 919` |
|   920 | child-process parent PID probe                                           | `tools/run-node-tests.sh --stage 920` |
|   921 | upstream process parent PID fixture                                      | `tools/run-node-tests.sh --stage 921` |
|   922 | process next-tick argument validation                                    | `tools/run-node-tests.sh --stage 922` |
|   923 | child-process spawn surface                                              | `tools/run-node-tests.sh --stage 923` |
|   924 | process source-map enablement validation                                 | `tools/run-node-tests.sh --stage 924` |
|   925 | process ref and unref hooks                                              | `tools/run-node-tests.sh --stage 925` |
|   926 | os signal constants                                                      | `tools/run-node-tests.sh --stage 926` |
|   927 | os fast host information surface                                         | `tools/run-node-tests.sh --stage 927` |
|   928 | os home-directory fallback surface                                       | `tools/run-node-tests.sh --stage 928` |
|   929 | broad os information surface                                             | `tools/run-node-tests.sh --stage 929` |
|   930 | events max-listener static APIs                                          | `tools/run-node-tests.sh --stage 930` |
|   931 | events static listener inspection                                        | `tools/run-node-tests.sh --stage 931` |
|   932 | EventEmitter event name enumeration                                      | `tools/run-node-tests.sh --stage 932` |
|   933 | upstream EventEmitter list contract                                      | `tools/run-node-tests.sh --stage 933` |
|   934 | static events listener count                                             | `tools/run-node-tests.sh --stage 934` |
|   935 | static events listener count validation                                  | `tools/run-node-tests.sh --stage 935` |
|   936 | static events max-listener validation                                    | `tools/run-node-tests.sh --stage 936` |
|   937 | static events max-listener target validation                             | `tools/run-node-tests.sh --stage 937` |
|   938 | events abort-listener disposable contract                                | `tools/run-node-tests.sh --stage 938` |
|   939 | events abort-listener argument validation                                | `tools/run-node-tests.sh --stage 939` |
|   940 | events static symbol and rejection settings                              | `tools/run-node-tests.sh --stage 940` |
|   941 | EventEmitter listener enumeration                                        | `tools/run-node-tests.sh --stage 941` |
|   942 | EventEmitter off alias lifecycle                                         | `tools/run-node-tests.sh --stage 942` |
|   943 | EventEmitter remove-all-listeners cleanup                                | `tools/run-node-tests.sh --stage 943` |
|   944 | EventEmitter prepend-listener ordering                                   | `tools/run-node-tests.sh --stage 944` |
|   945 | EventEmitter instance max-listener APIs                                  | `tools/run-node-tests.sh --stage 945` |
|   946 | EventEmitter raw ordinary listener inspection                            | `tools/run-node-tests.sh --stage 946` |
|   947 | EventEmitter prepend-once listener ordering                              | `tools/run-node-tests.sh --stage 947` |
|   948 | EventEmitter once-listener introspection                                 | `tools/run-node-tests.sh --stage 948` |
