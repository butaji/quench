### Test262 no-jit: areas short of the 20% margin

| area | cases | CPU s q / n / b | CPU q÷n | CPU q÷b | RSS max MiB q / n / b |
|---|---|---|---|---|---|
| annexB/built-ins | 241 | 4.0 / 12.8 / 4.8 | 0.31 | 0.83 | 30 / 186 / 47 |
| built-ins/Array | 3081 | 29.0 / 101.7 / 38.2 | 0.29 | 0.76 | 92 / 57 / 36 |
| built-ins/ArrayBuffer | 221 | 2.3 / 7.6 / 2.7 | 0.30 | 0.84 | 18 / 44 / 23 |
| built-ins/Atomics | 389 | 53.0 / 14.3 / 5.2 | 3.70 | 10.24 | 20 / 44 / 23 |
| built-ins/RegExp | 1879 | 133.9 / 314.0 / 104.2 | 0.43 | 1.28 | 49 / 121 / 51 |
| built-ins/String | 1223 | 18.9 / 38.9 / 14.2 | 0.49 | 1.33 | 18 / 45 / 23 |
| built-ins/TypedArray | 1446 | 51.6 / 53.8 / 23.3 | 0.96 | 2.22 | 1696 / 76 / 39 |
| built-ins/TypedArrayConstructors | 738 | 7.3 / 25.5 / 10.1 | 0.29 | 0.73 | 25 / 45 / 28 |
| built-ins/decodeURI | 55 | 9.7 / 19.9 / 4.3 | 0.49 | 2.26 | 31 / 44 / 91 |
| built-ins/decodeURIComponent | 56 | 9.6 / 20.5 / 4.3 | 0.47 | 2.22 | 31 / 44 / 91 |
| built-ins/encodeURI | 31 | 0.9 / 2.1 / 0.6 | 0.43 | 1.48 | 21 / 44 / 39 |
| built-ins/encodeURIComponent | 31 | 0.8 / 2.0 / 0.6 | 0.39 | 1.24 | 20 / 44 / 39 |
| built-ins/parseFloat | 54 | 0.6 / 2.1 / 0.7 | 0.27 | 0.82 | 23 / 43 / 35 |
| built-ins/parseInt | 55 | 0.6 / 2.1 / 0.7 | 0.28 | 0.90 | 22 / 43 / 35 |
| language/comments | 51 | 3.9 / 4.4 / 1.5 | 0.88 | 2.57 | 149 / 168 / 47 |
| language/expressions | 11033 | 80.7 / 367.0 / 136.7 | 0.22 | 0.59 | 37 / 47 / 32 |
| language/identifiers | 268 | 29.8 / 9.1 / 3.9 | 3.27 | 7.55 | 21 / 48 / 43 |
| staging/decorators | 3 | 0.0 / 0.1 / 0.0 | 0.31 | 0.92 | 13 / 44 / 22 |
| staging/sm | 1406 | 66.8 / 89.1 / 71.8 | 0.75 | 0.93 | 536 / 1556 / 4247 |

### Test262 jit: areas short of the 20% margin

| area | cases | CPU s q / n / b | CPU q÷n | CPU q÷b | RSS max MiB q / n / b |
|---|---|---|---|---|---|
| annexB/built-ins | 241 | 4.2 / 13.8 / 4.9 | 0.31 | 0.86 | 30 / 195 / 98 |
| built-ins/Array | 3081 | 28.9 / 106.3 / 41.6 | 0.27 | 0.70 | 92 / 61 / 46 |
| built-ins/Atomics | 389 | 53.0 / 14.4 / 5.7 | 3.67 | 9.29 | 20 / 48 / 28 |
| built-ins/RegExp | 1879 | 134.0 / 153.2 / 61.5 | 0.87 | 2.18 | 49 / 137 / 56 |
| built-ins/String | 1223 | 19.2 / 41.6 / 16.0 | 0.46 | 1.21 | 18 / 53 / 25 |
| built-ins/TypedArray | 1446 | 47.5 / 55.5 / 26.6 | 0.85 | 1.78 | 1698 / 86 / 52 |
| built-ins/decodeURI | 55 | 9.4 / 40.7 / 3.0 | 0.23 | 3.16 | 31 / 61 / 84 |
| built-ins/decodeURIComponent | 56 | 9.5 / 40.7 / 3.1 | 0.23 | 3.03 | 31 / 61 / 82 |
| built-ins/encodeURI | 31 | 0.9 / 2.6 / 0.6 | 0.35 | 1.50 | 21 / 61 / 48 |
| built-ins/encodeURIComponent | 31 | 0.9 / 2.5 / 0.6 | 0.35 | 1.45 | 21 / 59 / 48 |
| built-ins/parseInt | 55 | 0.7 / 2.4 / 0.8 | 0.27 | 0.84 | 23 / 54 / 41 |
| language/comments | 51 | 3.9 / 5.3 / 1.5 | 0.75 | 2.65 | 169 / 177 / 105 |
| language/expressions | 11033 | 80.3 / 379.4 / 146.8 | 0.21 | 0.55 | 37 / 51 / 38 |
| language/identifiers | 268 | 29.1 / 9.4 / 4.1 | 3.10 | 7.05 | 21 / 51 / 44 |
| staging/decorators | 3 | 0.0 / 0.1 / 0.0 | 0.30 | 0.83 | 13 / 48 / 22 |
| staging/sm | 1406 | 67.5 / 82.3 / 58.1 | 0.82 | 1.16 | 536 / 1560 / 2241 |

