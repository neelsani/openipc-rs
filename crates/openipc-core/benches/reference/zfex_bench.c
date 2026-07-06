#include "zfex.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { K = 8, N = 12, BLOCK = 3996, SAMPLES = 9, ITERATIONS = 100000 };

static uint64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC_RAW, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static int compare_double(const void *lhs, const void *rhs) {
    const double a = *(const double *)lhs;
    const double b = *(const double *)rhs;
    return (a > b) - (a < b);
}

static double measure(fec_t *fec, uint8_t **blocks, unsigned missing) {
    double samples[SAMPLES];
    volatile uint8_t checksum = 0;

    for (unsigned sample = 0; sample < SAMPLES; ++sample) {
        const uint64_t start = now_ns();
        for (unsigned iteration = 0; iteration < ITERATIONS; ++iteration) {
            const uint8_t *inputs[K];
            uint8_t *outputs[K];
            unsigned indexes[K];
            unsigned input = 0;
            unsigned output = 0;

            for (unsigned primary = 0; primary < K - missing; ++primary) {
                inputs[input] = blocks[primary];
                indexes[input++] = primary;
            }
            for (unsigned parity = 0; parity < missing; ++parity) {
                inputs[input] = blocks[K + parity];
                indexes[input++] = K + parity;
                outputs[output++] = blocks[K - missing + parity];
            }
            if (fec_decode_simd(fec, inputs, outputs, indexes, BLOCK) != ZFEX_SC_OK) {
                abort();
            }
            checksum ^= outputs[0][iteration % BLOCK];
        }
        samples[sample] = (double)(now_ns() - start) / ITERATIONS;
    }

    qsort(samples, SAMPLES, sizeof(samples[0]), compare_double);
    if (checksum == 0xff) {
        fprintf(stderr, "checksum=%u\n", checksum);
    }
    return samples[SAMPLES / 2];
}

int main(void) {
    fec_t *fec = NULL;
    if (fec_new(K, N, &fec) != ZFEX_SC_OK || fec == NULL) {
        return 1;
    }

    uint8_t *blocks[N];
    for (unsigned i = 0; i < N; ++i) {
        if (posix_memalign((void **)&blocks[i], ZFEX_SIMD_ALIGNMENT,
                          ZFEX_ROUND_UP_SIMD(BLOCK)) != 0) {
            return 2;
        }
        memset(blocks[i], (int)(i * 31u), ZFEX_ROUND_UP_SIMD(BLOCK));
    }
    if (fec_encode_simd(fec, (const uint8_t **)blocks, blocks + K, BLOCK) !=
        ZFEX_SC_OK) {
        return 3;
    }

    printf("reference_zfex_one_missing %.2f ns/op\n", measure(fec, blocks, 1));
    printf("reference_zfex_four_missing %.2f ns/op\n", measure(fec, blocks, 4));

    for (unsigned i = 0; i < N; ++i) {
        free(blocks[i]);
    }
    fec_free(fec);
    return 0;
}
