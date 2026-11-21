// jetstream-row: argon2-wasm
// jetstream-files: wasm/argon2/build/argon2.js wasm/argon2/benchmark.js
// iterations: 30
// jetstream-driver: class=WasmEMCC preload=wasmBinary=wasm/argon2/build/argon2.wasm.z deterministic=true
// c-sources: wasm/argon2/src/blake2/blake2b.c wasm/argon2/src/argon2.c wasm/argon2/src/core.c wasm/argon2/src/encoding.c wasm/argon2/src/thread.c wasm/argon2/src/opt.c -Iwasm/argon2/include -DARGON2_NO_THREADS
/* C analogue of the argon2-wasm row: the argon2 library sources the row's Wasm was built from
 * (without the SIMD flags only the Wasm build needs), driven as its benchmark.js does: each
 * iteration hashes and verifies the nine passwords with Argon2id (t=3, m=1024 KiB, p=1). Salts
 * come from a fixed xorshift generator, where the row uses its seeded Math.random. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "argon2.h"

enum { ITERATIONS = 30, T_COST = 3, M_COST = 1024, PARALLELISM = 1, SALT_LENGTH = 12, HASH_LENGTH = 24 };
#define ARGON2_VERSION_1_3 0x13
static const argon2_type ARGON2_TYPE = Argon2_id;

static const char *PASSWORDS[] = {
    "123456", "p@assw0rd", "qwerty", "letmein", "汉字漢字", "كلمة المرور",
    "Z7ihQxGE93", "0pjTjkrnsM", "6Kg3AmWnVc",
};
enum { PASSWORD_COUNT = sizeof PASSWORDS / sizeof PASSWORDS[0] };

static uint32_t rng_state = 2463534242u;

static uint32_t next_random(void) {
    rng_state ^= rng_state << 13;
    rng_state ^= rng_state >> 17;
    rng_state ^= rng_state << 5;
    return rng_state;
}

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

static int check(const char *what, int status) {
    if (status == ARGON2_OK)
        return 0;
    fprintf(stderr, "%s exited with status: %d (%s)\n", what, status, argon2_error_message(status));
    return 1;
}

static int hash_and_verify(const char *password) {
    size_t length = strlen(password);
    size_t encoded_size = argon2_encodedlen(T_COST, M_COST, PARALLELISM, SALT_LENGTH, HASH_LENGTH, ARGON2_TYPE) + 1;
    unsigned char salt[SALT_LENGTH], hash[HASH_LENGTH];
    char *encoded = malloc(encoded_size);
    for (int i = 0; i < SALT_LENGTH; i++)
        salt[i] = (unsigned char)next_random();
    int status = argon2_hash(T_COST, M_COST, PARALLELISM, password, length, salt, SALT_LENGTH, hash, HASH_LENGTH,
                             encoded, encoded_size, ARGON2_TYPE, ARGON2_VERSION_1_3);
    if (check("argon2_hash", status)) {
        free(encoded);
        return 1;
    }
    status = argon2_verify(encoded, password, length, ARGON2_TYPE);
    free(encoded);
    return check("argon2_verify", status);
}

int main(int argc, char **argv) {
    int iterations = argc > 1 ? atoi(argv[1]) : ITERATIONS;
    double start = now_ms();
    for (int i = 0; i < iterations; i++) {
        for (int p = 0; p < PASSWORD_COUNT; p++) {
            if (hash_and_verify(PASSWORDS[p])) {
                printf("validation: FAILED in iteration %d\n", i);
                return 1;
            }
        }
    }
    double elapsed = now_ms() - start;
    printf("argon2-wasm: %.3f ms for %d iterations\n", elapsed, iterations);
    printf("validation: ok (every hash verified, %d passwords per iteration)\n", PASSWORD_COUNT);
    return 0;
}
