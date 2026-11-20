// jetstream-row: gbemu
// jetstream-files: Octane/gbemu-part1.js Octane/gbemu-part2.js
// iterations: 120
// jetstream-driver: deterministic=true
/* C analogue of JetStream's Octane/gbemu-part1.js and gbemu-part2.js: a Game Boy Color
 * emulator (Grant Galitz's GameBoyCore, adapted by Google as a benchmark) that runs the
 * embedded cartridge for 250,000 instructions and checks the final processor and memory
 * state against the JS expectation (checkFinalState).
 *
 * Ported, and matched against the JS by instruction trace (every executed opcode with its
 * registers and flags, plus the LCD, interrupt and I/O state): the SM83 CPU with the cycle
 * counts of the JS TICKTable, the interrupt latch and EI/RETI delay (IRQLineMatched is
 * refreshed only at the points where the JS calls checkIRQMatching), the DIV and TIMA
 * timers, the memory map (ROM, MBC1, external RAM, WRAM and echo, OAM with mode gating,
 * I/O, HRAM, IE), OAM DMA, the LCD line state machine with STAT, LYC and VBlank, the CGB
 * parts the cartridge uses (speed switch through KEY1, VRAM bank 1, tile maps, palette
 * index and data ports, HDMA with its CPU cost, the NR52 power-off clear), and the
 * background scanline renderer.
 *
 * Not ported: the audio output, the sprite and window pixels (sprites still count toward
 * the mode 3 length), MBC2/3/5/7, the real-time clock, serial transfers, the WRAM bank
 * select (the cartridge never writes the banked 0xD000 region in this run), the DMG mode,
 * and the JS JIT caches. The JS benchmark also validates a hash of the rendered frames
 * (validate(), EXPECTED_RESULT_HASH); that hash needs the full pixel renderer, so this port
 * validates the final state only. sumVRam is 0 in the JS because checkFinalState reads an
 * undefined property (gameboy.VRam). The ROM image is the base64 literal from the JS source,
 * decoded with the same algorithm the JS falls back to (see decode_rom); that decoder
 * appends two bytes for the final "==" group, which the expected sums include.
 *
 * main() does one warm-up run, then ITERATIONS timed runs, and prints the elapsed
 * milliseconds and a validation line. It exits 1 when the final state differs. */
#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/* The cycle accounting is double arithmetic; the build does not use -march flags, so no FMA
 * contraction happens and the results agree with the JS. */

#define ITERATIONS 120
#define TOTAL_INSTRUCTIONS 250000L
/* settings[6] = 4: baseCPUCyclesPerIteration = 0x80000 / 0x7D * 4 = 16777.216,
 * CPUCyclesTotal starts at the whole part minus the roundoff, 16776. */
#define CYCLES_BASE 16776L
#define EXPECTED_STATE \
  "{\"registerA\":160,\"registerB\":255,\"registerC\":255,\"registerE\":11," \
  "\"registersHL\":51600,\"programCounter\":24309,\"stackPointer\":49706," \
  "\"sumROM\":10171578,\"sumMemory\":3435856,\"sumMBCRam\":234598,\"sumVRam\":0}"

#define FZ 0x80
#define FN 0x40
#define FH 0x20
#define FC 0x10

/* Generated from Octane/gbemu-part2.js by a script: cycle tables, post-boot I/O dump, ROM image. */
static const uint8_t TICK_TABLE[256] = {
  4, 12, 8, 8, 4, 4, 8, 4, 20, 8, 8, 8, 4, 4, 8, 4,
  4, 12, 8, 8, 4, 4, 8, 4, 12, 8, 8, 8, 4, 4, 8, 4,
  8, 12, 8, 8, 4, 4, 8, 4, 8, 8, 8, 8, 4, 4, 8, 4,
  8, 12, 8, 8, 12, 12, 12, 4, 8, 8, 8, 8, 4, 4, 8, 4,
  4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4,
  4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4,
  4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4,
  8, 8, 8, 8, 8, 8, 4, 8, 4, 4, 4, 4, 4, 4, 8, 4,
  4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4,
  4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4,
  4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4,
  4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4,
  8, 12, 12, 16, 12, 16, 8, 16, 8, 16, 12, 0, 12, 24, 8, 16,
  8, 12, 12, 4, 12, 16, 8, 16, 8, 16, 12, 4, 12, 4, 8, 16,
  12, 12, 8, 4, 4, 16, 8, 16, 16, 4, 16, 4, 4, 4, 8, 16,
  12, 12, 8, 4, 4, 16, 8, 16, 12, 8, 16, 4, 0, 4, 8, 16,
};

static const uint8_t TICK_TABLE_CB[256] = {
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 12, 8,
  8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 12, 8,
  8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 12, 8,
  8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 12, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
  8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8,
};

static const uint8_t FFXX_DUMP[256] = {
  15, 0, 124, 255, 0, 0, 0, 248, 255, 255, 255, 255, 255, 255, 255, 1,
  128, 191, 243, 255, 191, 255, 63, 0, 255, 191, 127, 255, 159, 255, 191, 255,
  255, 0, 0, 191, 119, 243, 241, 255, 255, 255, 255, 255, 255, 255, 255, 255,
  0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255,
  145, 128, 0, 0, 0, 0, 0, 252, 0, 0, 0, 0, 255, 126, 255, 254,
  255, 255, 255, 255, 255, 255, 62, 255, 255, 255, 255, 255, 255, 255, 255, 255,
  255, 255, 255, 255, 255, 255, 255, 255, 192, 255, 193, 0, 254, 255, 255, 255,
  248, 255, 0, 0, 0, 143, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255,
  206, 237, 102, 102, 204, 13, 0, 11, 3, 115, 0, 131, 0, 12, 0, 13,
  0, 8, 17, 31, 136, 137, 0, 14, 220, 204, 110, 230, 221, 221, 217, 153,
  187, 187, 103, 99, 110, 14, 236, 204, 221, 220, 153, 159, 187, 185, 51, 62,
  69, 236, 82, 250, 8, 183, 7, 93, 1, 253, 192, 255, 8, 252, 0, 229,
  11, 248, 194, 206, 244, 249, 15, 127, 69, 109, 61, 254, 70, 151, 51, 94,
  8, 239, 241, 255, 134, 131, 36, 116, 18, 252, 0, 159, 180, 183, 6, 213,
  208, 122, 0, 158, 4, 95, 65, 47, 29, 119, 54, 117, 129, 170, 112, 58,
  152, 209, 113, 2, 77, 1, 193, 255, 13, 0, 211, 5, 249, 0, 11, 0,
};

/* gameboy_rom: standard base64 of the cartridge, as in the JS source. */
static const char ROM_B64[] =
  "r+BPyZiEZwA+AeBPySAobeEq6gAgKlYj5WJv6SRmZjjhKuXqACDJ/////////////////////////////////xgHZwCYhGcA"
  "2fX6/3/1xdXlIRPKNgHN9f/h0cHx6gAg+hLKtyAC8cnwgLcoF/CC7hjgUT6Q4FOv4FLgVOCAPv/gVfHZ8IG3IALx2fBA7gjg"
  "QA8PD+YB7gHgT/CC4FHuEOCCPojgU6/gUuBU4IE+/uBV4ID6NMs86jTL8dkKCgoKbWFkZSBieSBhZ28uIGVtYWlsOmdvYnV6"
  "b3ZAeWFob28uY29tCnVybDogc3BlY2N5LmRhLnJ1CgoKCv///////wDDSgnO7WZmzA0ACwNzAIMADAANAAgRH4iJAA7czG7m"
  "3d3Zmbu7Z2NuDuzM3dyZn7u5Mz5BR08nUyBSRUFMVElNRSCAAAAAAgEDADMBSTQeIUD/y37I8P/1y4fg//BE/pEg+su+8eD/"
  "yT7A4EY+KD0g/cnF1eWvEQPK1RITEhMGAyEAyuXFTgYAIWAMCQkqEhMqEhPB4SMFIOrhrwYIzYsU4dHByf//////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "//////////8AAgMFBggJCwwOEBETFBYXGBobHR4fISIjJSYnKSorLC0uLzAxMjM0NTY3ODg5Ojo7PDw9PT4+Pj8/Pz9AQEBA"
  "QEBAQEBAPz8/Pz4+PT08PDs7Ojk5ODc2NTU0MzIxMC8uLCsqKSgmJSQjISAfHRwaGRcWFRMSEA8NCwoIBwUEAgH//fz6+ff2"
  "9PPx8O7t6+ro5+Xk4uHg3t3c2tnY19bU09LR0M/OzczLysnJyMfGxsXFxMPDw8LCwcHBwcDAwMDAwMDAwMDBwcHBwsLDw8PE"
  "xcXGxsfIycnKy8zNzs/Q0dLT1NXX2Nna3N3e4OHi5OXn6Onr7O7v8fL09vf5+vz9AAEECRAZJDFAUWR5kKnE4QAhRGmQueQR"
  "QHGk2RBJhMEAQYTJEFmk8UCR5DmQ6UShAGHEKZD5ZNFAsSSZEIkEgQCBBIkQmSSxQNFk+ZApxGEAoUTpkDnkkUDxpFkQyYRB"
  "AMGESRDZpHFAEeS5kGlEIQDhxKmQeWRRQDEkGRAJBAEAAQQJEBkkMUBRZHmQqcThACFEaZC55BFAcaTZEEmEwQBBhMkQWaTx"
  "QJHkOZDpRKEAYcQpkPlk0UCxJJkQiQSBAIEEiRCZJLFA0WT5kCnEYQChROmQOeSRQPGkWRDJhEEAwYRJENmkcUAR5LmQaUQh"
  "AOHEqZB5ZFFAMSQZEAkEAQAAAAAAAAAAAAAAAAAAAAABAQEBAQEBAgICAgIDAwMDBAQEBAUFBQUGBgYHBwcICAkJCQoKCgsL"
  "DAwNDQ4ODw8QEBEREhITExQUFRUWFxcYGRkaGhscHB0eHh8gISEiIyQkJSYnJygpKisrLC0uLzAxMTIzNDU2Nzg5Ojs8PT4/"
  "QEFCQ0RFRkdISUpLTE1OT1FSU1RVVldZWltcXV9gYWJkZWZnaWprbG5vcHJzdHZ3eXp7fX5/gYKEhYeIiouNjpCRk5SWl5ma"
  "nJ2foKKkpaepqqytr7GytLa3ubu9vsDCxMXHycvMztDS1NXX2dvd3+Hi5Obo6uzu8PL09vj6/P4A//z38Ofcz8CvnIdwVzwf"
  "AN+8l3BHHO/Aj1wn8Ld8PwC/fDfwp1wPwG8cx3AXvF8AnzzXcAecL8BP3Gfwd/x/AH/8d/Bn3E/AL5wHcNc8nwBfvBdwxxxv"
  "wA9cp/A3fL8AP3y38Cdcj8DvHEdwl7zfAB88V3CHnK/Az9zn8Pf8/wD//Pfw59zPwK+ch3BXPB8A37yXcEcc78CPXCfwt3w/"
  "AL98N/CnXA/AbxzHcBe8XwCfPNdwB5wvwE/cZ/B3/H8Af/x38GfcT8AvnAdw1zyfAF+8F3DHHG/AD1yn8Dd8vwA/fLfwJ1yP"
  "wO8cR3CXvN8AHzxXcIecr8DP3Ofw9/z/AP/////////////////////+/v7+/v79/f39/fz8/Pz8+/v7+vr6+vn5+fj4+Pf3"
  "9/b29fX19PTz8/Ly8fHw8PDv7u7t7ezs6+vq6uno6Ofn5uXl5OPj4uHh4N/e3t3c3Nva2djY19bV1NTT0tHQz8/OzczLysnI"
  "x8bFxMPCwcDAvr28u7q5uLe2tbSzsrGwr62sq6qpqKalpKOioJ+enZyamZiWlZSTkZCPjYyLiYiHhYSCgYB+fXt6eHd1dHJx"
  "cG5sa2loZmVjYmBfXVtaWFdVU1JQTk1LSUhGREJBPz08Ojg2NDMxLy0rKigmJCIgHx0bGRcVExEPDQsJBwUDAf9/Px8PBwMB"
  "gEAgEAgEAgEAAQEBAQEBAQEBAQEA//////////////+AEAcAAQABAAEBAAEBAAEA/wD//wD//wD/AP+AKwcBAAEAAQD/AP8A"
  "/wD/AP8A/wABAAEAAQCARgcBAQEBAQD//////////////wABAQEBAQGAYQf/////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "//////////////////////////////////////////+AwODw+Pz+/xEAwAGxwj4E9cU+BfUKbwMKZ37+gCALI34LAiN+AwIL"
  "GOsahhIDHBwcHPE9IN7BIRAAGVRdPgX1Cm8DCmcalhIjfAILfQIDAx0dHR3xPSDnIRgAGVRd8T0grskRAcAB6cI+BPUKbwMK"
  "Z37+gCALI34LAiN+AwILGOs+CvUahhIcHBwc8T0g9CN8Agt9AgMD8T0g0MkgIEZJTExFRCAgIFBPTFlHT05TIEhFTElDT1BU"
  "RVJJTiBBQ1RJT04gIQDADgpwLHQsGhPWICI2ACwNIPE+oOoQyngBCQDlYmsJVF3hDMYKR3AsdCwaG9YgIjYALA0g8a/qEcrJ"
  "+hDK/jDI1gTqEMpHPqCQ/lA4Aj5QDgAM1ggw+3ghAcARBAB3xggZDSD5+hHKg+oRykf+UDgCPlAOAAzWCDD7eC4td9YIGQ0g"
  "+ckh9grzMf/PzVABr+Am4P/gD+BD4EL2SOBFPkDgQT4E4AfN9RM+CuoAAA4HeeBwJqCvIstsKPsNIPIh/v8yy30g+wEKABH1"
  "/yFpAc3kE+cCAVYAEQDBIVt2zeQTrwYYIWsOzYsUIYsOzaQUxwGwAxEAgCGhF8XlzeQT4cERAIjN5BMhAJgRAwABYMDHcc9y"
  "IwUg+BQdIPHN9RMhuxUGAc2WE82JEz5E4EGv4EU+A+D/+z4B6hLK4E0QAAB4zccTBSD6zZATxwEACFkhAIhzIwt4sSD5IQDH"
  "Pv9FdyRwJCJ3JXclcCwg8x5/IQCYx3PPNgDL1DYIx3PLlCPLVCjuPoABDxARIAAhIpjF5XfL1HfLlDwZDSD1POEswQUg7D48"
  "6jPLr+o0yz3qL8s+oOCCPgLqG8vNiRM+ROBBr+BFPgPg/68+ACEXyyI+CiI+IHev6h7L4ITgluodyz4B6h/L6g/D6g3KBlAR"
  "nAjNxAjNcwsBLAHFzTsLzQAJwQt4sSDzzZATxwEACFkhAIhzIwt4sSD5zfUTeQYQIYMOzYsUPv/qKcsGgBGwCM3ECM2JEwEs"
  "AcXNbAzNAAnBC3ixIPOv6hLKzZATPpDgU/PHAbADEQCIIaEXzeQTzfUTIQIWBgHNlhPNiRM+ROBBr+BFPgPg//sY/j4D6gAg"
  "zcRGBgMhF8t+gCJ+gDwifoB3zckP+jDLb/oxy2fNtgs+AeCB8IG3IPv6Dcq3KAPNcwHJ+h3LBgARTg2Hb2AZKmZvTgkq4Itf"
  "KjzgjD1PKuCNe4eHg0cRAMUqEhwFIPp5h4eBRxEAxCoSHAUg+n3qMMt86jHLyfCL4I7wjOCP8I3gkBEAw9XlzcoQ4dHwpeaA"
  "EhwBAwAJ8JA94JAg6CEAxQYPKk+gXxq3IB95yzegXxq3IBYqT6BfGrcgD3nLN6BfGrcgBiwsLBhHLOXNyhDwlrcoKwYB8KXG"
  "P0/LfygBBcXwpMY/Vx4AzZMOe8H18KPGP1ceAM2TDsHhJCJwGAzhJPCjxj8i8KTGPyIsJRbDBg/wjj3gjsLiCz4C6gAgw1Jh"
  "fBjcHwAL7mpIYL9vBgMhF8t+gCJ+gDwifoB3zckPIcsNEQDGzf4MI+U+A+oAICEgy83+DPocy9YIb+ocy82vYAYDESDLIWIO"
  "xeXVzcoQ4fCjxhQi8KQiNg8jVF3hIyMjwQUg5M3ERsE+AeoAIAr+/ygiEQDGbyYAKRnlAwoDbyYAKRleI1bhKmZvxc0xHMwA"
  "QMEY2T4B4IHwgbcg+8l+PMjl1c3KEAYB8KVPy38oAQXF8KTLf/UoAi88Vx4AzZMO8XsgAi88xn/B9fCjy3/1KAIvPFceAM2T"
  "DvF7KAIvPMZ/wdESE3gSE+EjIyMYsFANAgAIDAYCRCgoFANEKAAUE0QAABQSRAAoFAJVKCjsA1UoAOwTVQAA7BJVACjsAAAE"
  "BQAAAAEFAAEBAwIGAQEDBwYCAgAHAwICAAcEAwMBAgYDAwEFBgQEAAECBAQAAwIFBQQFBgUFBAcGMgAAzgAAADIAAM4AAAAy"
  "AADOKAAAHhEAChEAAAAACu8AHu8AFAAKFAD2FAAPCgAF6AAC4gAQ3gAQ4gD+CgD74g4C3Q4C4QAC4vIC3fIC4AAM4PsM4PsQ"
  "4/sJ3fsJ/wABAQICAwMEBAUFAAAGAQYCBgMGBAYFBgAHAQcCBwMHBAcFBwYICQoKCwsMDA0NDgoPDxAQEQoSEhMTERQVFRYV"
  "FxUYCBkIGggb/yAAD/AbD/DlD/9//3+XEQAAAGD/f5cRAAAYAP9/lxEAAIB8lxH/f/9/QHz/f18IAADLI8sSeC9HeS9PAyEA"
  "AH2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPL"
  "En2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPL"
  "En2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPL"
  "En2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPLEn2Pb3yPZwk4BWd9kW+3yxPL"
  "EssoyxkJ0BPJ+hfLJgJvfuCcLzzgnn3GQG9+4Jvgn6/gmOCZ4JrgneChPkDgl/oYy29OfcZAb0bFeOCgeeCizdMQ8KPgpvCk"
  "4KnwpeCsr+Cg4KI+QOChzdMQ8KPgp/Ck4KrwpeCtwXkvPOCgr+CheOCizdMQ8KPgmfCk4JzwpeCf8Kbgl/Cp4JrwrOCd8Kfg"
  "mPCq4JvwreCe+hnLJgJvTn3GQG9GxXjgoHkvPOChr+CizdMQ8KPgpvCk4KnwpeCswXngoHjgoa/gos3TEPCj4KfwpOCq8KXg"
  "ra/goOChPkDgos3TEPCj4JnwpOCc8KXgn/Cm4JfwqeCa8KzgnfCn4JjwquCb8K3gnskq4KAq4KEq4KLwl1/woCYGV8t6ICDL"
  "e3soJy88X3qTMAIvPG96g1YlXiVvfiVuZxl8LzwYH3ovPFfLeyjhey88X5IwAi88b3qDViVeJW9+JW5nGXxH8Jhf8KEmBlfL"
  "eiAgy3t7KCcvPF96kzACLzxveoNWJV4lb34lbmcZfC88GB96LzxXy3so4XsvPF+SMAIvPG96g1YlXiVvfiVuZxl8T/CZX/Ci"
  "JgZXy3ogIMt7eygnLzxfepMwAi88b3qDViVeJW9+JW5nGXwvPBgfei88V8t7KOF7LzxfkjACLzxveoNWJV4lb34lbmcZfICB"
  "4KPwml/woCYGV8t6ICDLe3soJy88X3qTMAIvPG96g1YlXiVvfiVuZxl8LzwYH3ovPFfLeyjhey88X5IwAi88b3qDViVeJW9+"
  "JW5nGXxH8Jtf8KEmBlfLeiAgy3t7KCcvPF96kzACLzxveoNWJV4lb34lbmcZfC88GB96LzxXy3so4XsvPF+SMAIvPG96g1Yl"
  "XiVvfiVuZxl8T/CcX/CiJgZXy3ogIMt7eygnLzxfepMwAi88b3qDViVeJW9+JW5nGXwvPBgfei88V8t7KOF7LzxfkjACLzxv"
  "eoNWJV4lb34lbmcZfICB4KTwnV/woCYGV8t6ICDLe3soJy88X3qTMAIvPG96g1YlXiVvfiVuZxl8LzwYH3ovPFfLeyjhey88"
  "X5IwAi88b3qDViVeJW9+JW5nGXxH8J5f8KEmBlfLeiAgy3t7KCcvPF96kzACLzxveoNWJV4lb34lbmcZfC88GB96LzxXy3so"
  "4XsvPF+SMAIvPG96g1YlXiVvfiVuZxl8T/CfX/CiJgZXy3ogIMt7eygnLzxfepMwAi88b3qDViVeJW9+JW5nGXwvPBgfei88"
  "V8t7KOF7LzxfkjACLzxveoNWJV4lb34lbmcZfICB4KXJ9T6D4EDxyfWv4EDxyfXF1eXHKv7/KFD+FiAaTiMqh4eHVF1vJgAp"
  "KXgGmAlHelRne11vGNzGYBLPeBIcGNN2ACETyjQ1KPc1yfvFBmR2AAUg+8HJ+3YABSD7yfXF1eUqEhMLeLEg+OHRwfHJxeUB"
  "AKAhAMDNAxThwcnF5XEjBSD74cHJxdXlAQCAIZXKzQMU4dHBycXV5a/qFcuwIAwaEyIaEzIEDXjqFcvlxRq+EyAPIxq+IAkT"
  "Iw0gCMHhGBkrGyMjBSDmecFPBBoTIhoTIiEVyzThDSDS+hXL4dHBydVfzXIUuzD60cnF9cH6FMrLD6mAR/CLkR+AR/AFqOoU"
  "ysHJ9cXltxcXF/aA4Ggq4GkFIPo+5OBH4cHxyfXF5bcXFxf2gOBqKuBrBSD6PuTgSOBJ4cHxyT4Q4ADwAC/LN+bwRz4g4ADw"
  "AC/mD7DqFsvJzyEAgK8GIE8+CCINIPwFIPnHIQCABiBPIg0g/AUg+cnFzQMVSs0eFcHJxc0RFUjNGRVLzSMVwcnFBgHNKxXB"
  "ycUGABj2xQYDGPHFBgLNKxXByfXlh4eAJsBvceHxyfXlh4cmwG9GI04jXiNW4fHJ9cXV5eCDKjzK8BPWIF/wg835FF95xghP"
  "ezwY6PXF1eXF1c13FdHBex4FIS3LGNUBKssR8NjNlRURGPzNlRURnP/NlRUR9v/NlRUR//8+LzwZOPwCA3ovV3svXxMZyTAw"
  "RlBT/zAwUE5UU/8wMExJTkVT/xYFB1dFTENPTUUgVE8WBQgtUkVBTFRJTUUtFgAJREVNTyBNQURFIEVTUEVDSUFMTFkWAQpG"
  "T1IgTENQJzIwMDAgUEFSVFn/FgAAR1JFRVRJTlg6ICAgICAgICAgICAWAAFEU0MsUEFOLFNBQixGQVRBTElUWRYAAkpFRkYg"
  "RlJPSFdFSU4sSUNBUlVTFgADRE9YLFFVQU5HLEFCWVNTICAgICAWAAQgICAgICAgICAgICAgICAgICAgIBYABUNSRURJVFM6"
  "ICAgICAgICAgICAgFgAGQUxMIEdGWCZDT0RFIEJZIEFHTyAWAAdIRUxJQ09QVEVSIDNEIE1PREVMIBYACENSRUFURUQgQlkg"
  "QlVTWSAgICAgFgAJICAgICAgICAgICAgICAgICAgICAWAApVU0VEIFNPRlRXQVJFOiAgICAgIBYAC1JHQkRTLE5PJENBU0gs"
  "RkFSICAgFgAMICAgICAgICAgICAgICAgICAgICAWAA1DT05UQUNUOiAgICAgICAgICAgIBYADkdPQlVaT1ZAWUFIT08uQ09N"
  "ICAgFgAPSFRUUDovL1NQRUNDWS5EQS5SVSAWABAgICAgICAgICAgICAgICAgICAgIBYAEVNFRSBZT1UgT04gR0JERVYyMDAw"
  "/wAAAAAAAAAAAAAAAAAAAAAICBwUHBQ4KDgoMDBwUCAgKCh8VHxUKCgAAAAAAAAAABQUPip/QT4qfFT+gnxUKCgICDw0fkL8"
  "rP6Cfmr8hHhYJCR+Wn5SPCR4SPyU/LRISBgYPCR+Wjwkflr8tH5KNDQQEDgocFAgIAAAAAAAAAAABAQOChwUOCg4KDgoHBQI"
  "CBAQOCgcFBwUHBQ4KHBQICAAABQUPio8NH5CPCx8VCgoAAAICBwUPDR+QjwsOCgQEAAAAAAAAAAAEBA4KHBQcFAAAAAAAAB8"
  "fP6CfHwAAAAAAAAAAAAAAAAwMHhIeEgwMAQEDgoeEjwkeEjwkOCgQEAYGDwkflr+qv6q/LR4SDAwGBg8JHxUPDQ4KHxs/oJ8"
  "fBwcPiJ+Wjw0eEj8vP6CfHwcHD4iflo8NE5K/LR4SDAwJCR+Wn5afFT8tP6CfGwQEBwcPiJ8XPyEfnr8tHhIMDAYGDwkeFj8"
  "pP66/LR4SDAwPDx+Qv66XFQ4KHBQcFAgIBwcPiJ+Wjwkflr8tPiIcHAcHD4iflr+sn5KfHT4iHBwAAAAAAgIHBQICBAQOCgQ"
  "EAAACAgcFAgIEBA4KDgocFAAAAAAHBQ4KHBQcFA4KAAAAAAAADw8fkJ8fPyEeHgAAAAAAAA4KBwUHBQ4KHBQAAAYGDwkflr8"
  "tHhoEBA4KBAQHBw+In5a/rL8pPi4+IhwcBwcPiJ+Wv66/oL+uvy0SEg4OHxEflr8pP6a/LT4iHBwHBw+In5a5qbgoP6y/Ixw"
  "cDAweEh8VH5a7qr+uvyEeHgcHD4ifFx8RHhY/Lz+gnx8HBw+Inxc/IT4uOCg4KBAQBwcPiJ+Wvy8/qL+uvyEeHgkJH5a/rr+"
  "gv66/LT8tEhIPDx+QjwsOChwUHhY/IR4eDw8fkI+Og4KXFT8tHhIMDAkJH5afFR+Qv66/LT8tEhIICBwUHBQ4KDkpP66fEQ4"
  "OCgofFR+Qv6q/rr8tPy0SEgkJH5a/pr+qv6y7qr8tEhIHBw+In5a7qruqvy0+IhwcBwcPiJ+Wv66/IT4uOCgQEAcHD4iflr+"
  "uv6q/LT+inZ2HBw+In5a/LT4iPy0/LRISBwcPiJ8XP6Cfnr8tPiIcHB8fP6CfGw4KHBQcFBwUCAgJCR+Wn5a7qruqvy0eEgw"
  "MERE7qruqnxUfFR4SHBQICAkJH5aflr+uv6q/KR8VCgoJCR+WnxUOCg8JH5a/LRISCQkflr8tPy0eEhwUHBQICA8PH5C/LT4"
  "6Dwsflr8hHh4HBw+IjwsOChwUHhYfEQ4OEBA4KDwkHhIPCQeEg4KBAQ4OHxEPDQcFDgoeGj4iHBwGBg8JH5a7qpERAAAAAAA"
  "AAAAAAAAAAAAAAB8fP6CfHx81rdPfJJne5X1MAIvPEevyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxkw"
  "AYAfyxkwAYAfyxnLEcsXlDABhMsRyxeUMAGEyxHLF5QwAYTLEcsXlDABhMsRyxeUMAGEyxHLF5QwAYTLEcsXlDABhMsRyxeU"
  "MAGEeRcvT/F5MAIvPIVvJrcBAAA+t7zLEbrLED6/vcsRu8sQPj+8P8sRuj/LEL0/yxG7P8sQeLHIeKHAebcgB3xiV31rX3jL"
  "H9L/HD5AlU97lW96lPUwAi88R6/LGTABgB/LGTABgB/LGTABgB/LGTABgB/LGTABgB/LGTABgB/LGTABgB/LGTABgB/LGcsR"
  "yxeVMAGFyxHLF5UwAYXLEcsXlTABhcsRyxeVMAGFyxHLF5UwAYXLEcsXlTABhcsRyxeVMAGFyxHLF5UwAYV5Fy9P8XkwAi88"
  "hGcuQMMxHMsf0pcdPkCUT3qUZ3uV9TACLzxHr8sZMAGAH8sZMAGAH8sZMAGAH8sZMAGAH8sZMAGAH8sZMAGAH8sZMAGAH8sZ"
  "MAGAH8sZyxHLF5QwAYTLEcsXlDABhMsRyxeUMAGEyxHLF5QwAYTLEcsXlDABhMsRyxeUMAGEyxHLF5QwAYTLEcsXlDABhHkX"
  "L0/xeTACLzyFbyZAwzEcyx/SoRt91r9PfZNvepT1MAIvPEevyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAf"
  "yxkwAYAfyxkwAYAfyxnLEcsXlTABhcsRyxeVMAGFyxHLF5UwAYXLEcsXlTABhcsRyxeVMAGFyxHLF5UwAYXLEcsXlTABhcsR"
  "yxeVMAGFeRcvT/F5MAIvPIRnLr/DMRz/////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "/////////////////////////////////////////////////////3q8MAVUZ3tdb3u90pdAfZNPepRfkTA+V3nLPy88g+CD"
  "Pn+R5YdPbyYARCkpKQkBkVIJweV41kAXb3nWQB8fH+YPZ/CChGd55gcGB/YITwpP8INHLMl5S1+RV3nLPy88g+CDPneR5YdP"
  "byYARCkpKQkBklsJweV41kAXb3nWQB8fH+YPZ/CChGd55gcGB/YITwpP8INHLMmVT3qUX5EwPld5yz8vPIPggz5/keWHT28m"
  "AEQpKSkJAR9BCcHleNZAF2951kAfHx/mD2fwgoRneeYHBgf2CE8KT/CDRyzJeUtfkVd5yz8vPIPggz53keWHT28mAEQpKSkJ"
  "ASBKCcHleNZAF2951kAfHx/mD2fwgoRneeYHBgf2CE8KT/CDRyzJfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJ"
  "MAEkfrF3e8t4IAN6LCyAR8sJMAEkfrF3e8t4IAN6LCyAR8sJMAEkyX6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEk"
  "gEcALH6xInvLeCAGessJMAEkgEcALH6xInvLeCAGessJMAEkgEcALMl+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBH"
  "ywEwASV+sXd7y3ggA3osLIBHywEwASV+sXd7y3ggA3osLIBHywEwASXJfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEw"
  "ASWARywsfrF3e8t4IAZ6ywEwASWARywsfrF3e8t4IAZ6ywEwASWARywsyf//////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "/////////wHRe7o4A1pXewYHoE8K9XqgTwQK4JF6Hx8f5g9X8JNnTixGLXsfHx/mD1/wgoNne5LRxADB8JGiVy+mX3qhsyJ6"
  "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"
  "8JRfPniTZy5Hr8sdMAGEH8sdMAGEH8sdMAGEH8sdMAGEH8sdMAGEH8sdMAGEH8sdMAGEH8sdMAGEH8sdxkBnCA7KMQDC5fCV"
  "b8l7vTBVfZNPepRfkTAkV3nLPy88Rz5/kU3Fh09vJgBEKSkJAfdiCcHlJsLwkm94BoDJeUtfkVd5yz8vPEc+d5FNxYdPbyYA"
  "RCkpCQH4ZwnB5SbC8JJveAaAyZVPepRfkTAkV3nLPy88Rz5/kU3Fh09vJgBEKSkJAalsCcHlJsLwkm94BoDJeUtfkVd5yz8v"
  "PEc+d5FNxYdPbyYARCkpCQGqcQnB5SbC8JJveAaAyYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2D"
  "uDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gw"
  "BZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWT"
  "gnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4Jx"
  "LCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSws"
  "DYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2D"
  "uDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gw"
  "BZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWT"
  "gnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4Jx"
  "LCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSws"
  "DYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2D"
  "uDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gw"
  "BZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWT"
  "gnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4Jx"
  "LCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSws"
  "DYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2D"
  "uDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gw"
  "BZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWTgnEsLA2DuDAFk4JxLCwNg7gwBZOCcSwsDYO4MAWT"
  "gnEsLA2DuDAFk4JxLCwNyXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gw"
  "A5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOT"
  "gg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4IN"
  "cSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEs"
  "LIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyD"
  "uDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gw"
  "A5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOT"
  "gg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4IN"
  "cSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEs"
  "LIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyD"
  "uDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gw"
  "A5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOT"
  "gg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4IN"
  "cSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEs"
  "LIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyD"
  "uDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gw"
  "A5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDXEsLIO4MAOTgg1xLCyDuDADk4INcSwsg7gwA5OCDcmDuDAFk4Jx"
  "LCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSws"
  "DIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyD"
  "uDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gw"
  "BZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWT"
  "gnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4Jx"
  "LCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSws"
  "DIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyD"
  "uDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gw"
  "BZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWT"
  "gnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4Jx"
  "LCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSws"
  "DIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyD"
  "uDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gw"
  "BZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWT"
  "gnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4Jx"
  "LCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSws"
  "DIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyD"
  "uDAFk4JxLCwMg7gwBZOCcSwsDIO4MAWTgnEsLAyDuDAFk4JxLCwMg7gwBZOCcSwsDMlxLCyDuDADk4IMcSwsg7gwA5OCDHEs"
  "LIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyD"
  "uDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gw"
  "A5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOT"
  "ggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IM"
  "cSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEs"
  "LIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyD"
  "uDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gw"
  "A5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOT"
  "ggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IM"
  "cSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEs"
  "LIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyD"
  "uDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gw"
  "A5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOT"
  "ggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IM"
  "cSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEs"
  "LIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyDuDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggxxLCyD"
  "uDADk4IMcSwsg7gwA5OCDHEsLIO4MAOTggzJxg+Hh+oawXovpl96obMiei+mX3qgszIkeRgAInAtJCJwLSQicC0kInAtJCJw"
  "LSQicC0kInAtJCJwLSQicC0kInAtJCJwLSQicC0kInAtJCJwLSQW/8n/////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "//////////////////////////////////////////////////////////////////////////+qqlVVqqpVVaqqVVWqqlVV"
  "qqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVV"
  "qqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVV"
  "qqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVV"
  "qqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVACEzDDPAABIAEjPAMwwAIQAhMwwzwAASABIzwDMMACEAITMMM8AAEgASM8AzDAAh"
  "ACEzDDPAABIAEjPAMwwAIQAhMwwzwAASABIzwDMMACEAITMMM8AAEgASM8AzDAAhACEzDDPAABIAEjPAMwwAIQAhMwwzwAAS"
  "ABIzwDMMACEAITMMM8AAEgASM8AzDAAhACEzDDPAABIAEjPAMwwAIQAhMwwzwAASABIzwDMMACEAITMMM8AAEgASM8AzDAAh"
  "ACEzDDPAABIAEjPAMwwAIQAhMwwzwAASABIzwDMMACEAITMMM8AAEgASM8AzDAAhACEzDDPAABIAEjPAMwwAIQj8GH4y/WT7"
  "wO+B50CzINkI/Bh+Mv1k+8DvgedAsyDZCPwYfjL9ZPvA74HnQLMg2Qj8GH4y/WT7wO+B50CzINkI/Bh+Mv1k+8DvgedAsyDZ"
  "CPwYfjL9ZPvA74HnQLMg2Qj8GH4y/WT7wO+B50CzINkI/Bh+Mv1k+8DvgedAsyDZCPwYfjL9ZPvA74HnQLMg2Qj8GH4y/WT7"
  "wO+B50CzINkI/Bh+Mv1k+8DvgedAsyDZCPwYfjL9ZPvA74HnQLMg2Qj8GH4y/WT7wO+B50CzINkI/Bh+Mv1k+8DvgedAsyDZ"
  "CPwYfjL9ZPvA74HnQLMg2Qj8GH4y/WT7wO+B50CzINnMzMzMMzMzM8zMzMwzMzMzzMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMz"
  "zMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMzzMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMzzMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMz"
  "zMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMzzMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMzzMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMz"
  "zMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMzzMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMzzMzMzDMzMzPMzMzMMzMzM8zMzMwzMzMz"
  "wMDAwAwMDAzAwMDADAwMDMDAwMAMDAwMwMDAwAwMDAzAwMDADAwMDMDAwMAMDAwMwMDAwAwMDAzAwMDADAwMDMDAwMAMDAwM"
  "wMDAwAwMDAzAwMDADAwMDMDAwMAMDAwMwMDAwAwMDAzAwMDADAwMDMDAwMAMDAwMwMDAwAwMDAzAwMDADAwMDMDAwMAMDAwM"
  "wMDAwAwMDAzAwMDADAwMDMDAwMAMDAwMwMDAwAwMDAzAwMDADAwMDMDAwMAMDAwMwMDAwAwMDAzAwMDADAwMDMDAwMAMDAwM"
  "wMDAwAwMDAzAwMDADAwMDMDAwMAMDAwMwMDAwAwMDAzAwMDADAwMDPHxAQEBAQEBHx8QEBAQEBDx8QEBAQEBAR8fEBAQEBAQ"
  "8fEBAQEBAQEfHxAQEBAQEPHxAQEBAQEBHx8QEBAQEBDx8QEBAQEBAR8fEBAQEBAQ8fEBAQEBAQEfHxAQEBAQEPHxAQEBAQEB"
  "Hx8QEBAQEBDx8QEBAQEBAR8fEBAQEBAQ8fEBAQEBAQEfHxAQEBAQEPHxAQEBAQEBHx8QEBAQEBDx8QEBAQEBAR8fEBAQEBAQ"
  "8fEBAQEBAQEfHxAQEBAQEPHxAQEBAQEBHx8QEBAQEBDx8QEBAQEBAR8fEBAQEBAQ8fEBAQEBAQEfHxAQEBAQEPHxAQEBAQEB"
  "Hx8QEBAQEBCqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVV"
  "qqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVV"
  "qqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVV"
  "qqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlVVqqpVVaqqVVWqqlUC4XIscAl7InAJInAJInAJInAJInAJInAJ"
  "InAJInAJInAJInAJInAJInAJInAJInAJInAJLCwly2XIJGjJycnJyeEicAlyLHAJeyJwCSJwCSJwCSJwCSJwCSJwCSJwCSJw"
  "CSJwCSJwCSJwCSJwCSJwCSJwCSwsJctlyCRoycnJycnhInAJInAJcixwCXsicAkicAkicAkicAkicAkicAkicAkicAkicAki"
  "cAkicAkicAkicAksLCXLZcgkaMnJycnJ4SJwCSJwCSJwCXIscAl7InAJInAJInAJInAJInAJInAJInAJInAJInAJInAJInAJ"
  "InAJLCwly2XIJGjJycnJyeEicAkicAkicAkicAlyLHAJeyJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSwsJctl"
  "yCRoycnJycnhInAJInAJInAJInAJInAJcixwCXsicAkicAkicAkicAkicAkicAkicAkicAkicAkicAksLCXLZcgkaMnJycnJ"
  "4SJwCSJwCSJwCSJwCSJwCSJwCXIscAl7InAJInAJInAJInAJInAJInAJInAJInAJInAJLCwly2XIJGjJycnJyeEicAkicAki"
  "cAkicAkicAkicAkicAlyLHAJeyJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSwsJctlyCRoycnJycnhInAJInAJInAJInAJInAJ"
  "InAJInAJInAJcixwCXsicAkicAkicAkicAkicAkicAkicAksLCXLZcgkaMnJycnJ4SJwCSJwCSJwCSJwCSJwCSJwCSJwCSJw"
  "CSJwCXIscAl7InAJInAJInAJInAJInAJInAJLCwly2XIJGjJycnJyeEicAkicAkicAkicAkicAkicAkicAkicAkicAkicAly"
  "LHAJeyJwCSJwCSJwCSJwCSJwCSwsJctlyCRoycnJycnhInAJInAJInAJInAJInAJInAJInAJInAJInAJInAJInAJcixwCXsi"
  "cAkicAkicAkicAksLCXLZcgkaMnJycnJ4SJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCXIscAl7InAJInAJ"
  "InAJLCwly2XIJGjJycnJyeEicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAlyLHAJeyJwCSJwCSwsJctl"
  "yCRoycnJycnhInAJInAJInAJInAJInAJInAJInAJInAJInAJInAJInAJInAJInAJInAJcixwCXsicAksLCXLZcgkaMnJycnJ"
  "4SJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCSJwCXIscAl7LCwly2XIJGjJycnJydE+t5LI4IXm"
  "B8RSRPCFHx8focjlzTJE4XkicCwicCwicCwicCwicCwicCwicCwicCzJ+ABUXWhHeZAfyx1nATNZCfCCMQCv/qAoAzEAvwH/"
  "AOlHPgeQVF1HDjOvyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxkwAYAfyxlHIbRXCeViaz7/"
  "AQ8Ayfoay2/6G8uFZ/4UIAU+/y0YBtbsIAU8LOoby3zqGsvNr2AhlEbNyhDwpMagV/Cjxn9f1SGXRs3KEPCkxqBn8KPGf2/R"
  "zTEcKAsf2hhZH9oYWcPERny6OAViV31rX+XNmkbh1Xu90sxFe9ZA4Ih9k0884Il6lF/ghjzgij2RMGvgh3nLPy88g+CF8IIB"
  "DwBvVHzWQBfLN6GFZ3rmBxdvGAjwij3KAETgivCJX/CGV/CFGASCHSgLy38g+Ffwh4LghR3NYkUY2nvgifCIg1/l5gf2CG8m"
  "B1Z7aB8fHx/LHR/LHeYDxkBnrx7/6XnghpPgh3vLPy88geCF8IIBDwBvVHzWQBfLN6GFZ3rmBxdv8Ilf8IZX8IXLfyAHV/CH"
  "gh0YAYLghc1iRfCKPcoAROCKGN171kDgiHuVTzzgiXqUX+CGPOCKPZEwa+CHecs/LzyD4IXwggEPAG9UfNZAF8s3oYVneuYH"
  "F28YCPCKPcoAROCK8Ilf8IZX8IUYBIIdKAvLfyD4V/CHguCFHc0qRhjae+CJ8IiTX+XmB/YQbyYHVntoHx8fH8sdH8sd5gPG"
  "QGc+/1jpeeCGk+CHe8s/LzyB4IXwggEPAG9UfNZAF8s3oYVneuYHF2/wiV/whlfwhct/IAdX8IeCHRgBguCFzSpG8Io9ygBE"
  "4IoY3UYAALoAAHzWQMhPHx8f5h9HeeYHKAsE/gUwBvUhylblBT4PkCHJRoRn5fCCZ69vyfCCZ69vIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIi"
  "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIsnxAQ8APcqEVz0odj0oOj0idwkidwki"
  "dwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwksLCXLZSgCJGgidwkidwkidwkidwkidwkidwkidwki"
  "dwkidwkidwkidwkidwkidwkidwkidwkidwksLCXLZSgCJGgidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwki"
  "dwkidwkidwkidwksLCXLZSgCJGgidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkidwkid8kicAki"
  "cAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAksLCUicAkicAkicAkicAkicAkicAkicAkicAki"
  "cAkicAkicAkicAkicAkicAkicAkicAksLCUicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAki"
  "cAksLCUicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAksLCUicAkicAkicAkicAkicAki"
  "cAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAksLCUicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAki"
  "cAkicAkicAkicAksLCUicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAkicAloyfgAVF3wgjEA"
  "r/6gKAMxAL8B/wDFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXF"
  "xcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcXFxcVia/nJJgJ+4JovPOCY"
  "fcZAb37gl+Cbr+CZ4JzgneCePkDgn8n/////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////////////////////////////////////////////////////////////////////////////////"
  "////////////////////Aw=="
;

/* ---- processor and memory state ---- */

static uint8_t reg_a, reg_f, reg_b, reg_c, reg_d, reg_e, reg_h, reg_l;
static uint16_t sp, pc;
static int ime, ei_delay, halted, skip_pc_inc;
static int irq_line; /* JS IRQLineMatched: latched by check_irq, cleared when an IRQ launches */
/* IF and IE are held apart from memory, as in the JS (interruptsRequested, interruptsEnabled). */
static int if_reg, ie_reg;
static int sound_on; /* JS soundMasterEnabled (NR52 bit 7) */
static int hdma_running, vram_bank; /* JS hdmaRunning and currVRAMBank (FF55, FF4F) */
static uint8_t vram1[0x2000];       /* VRAM bank 1 (JS VRAM) */
static uint8_t bgchr[2][0x800];      /* tile maps at 0x9800-0x9FFF: JS BGCHRBank1 (bank 0), BGCHRBank2 (bank 1) */
static void dma_write(int tiles);
static uint8_t bg_raw_palette[64], obj_raw_palette[64]; /* JS gbcBGRawPalette / gbcOBJRawPalette */

static long cpu_ticks;

static uint8_t memory[0x10000];
static uint8_t mbc_ram[0x8000];
static uint8_t *rom;
static long rom_len;
static long rom_base;   /* JS currentROMBank: ROM index of 0x4000 minus 0x4000 */
static long ram_pos;    /* JS currMBCRAMBankPosition */
static int ram_enabled, mbc1_mode, rom_offs;

static int actual_line, mode_stat, stat_tracker, lcd_on, sprite_count;
static int mode0_trig, mode1_trig, mode2_trig, lyc_trig;
static long lcd_ticks;
static uint32_t frame[160 * 144];

static long div_ticks, timer_ticks, tac_clocker;
static int tima_enabled;

static double emulator_ticks, cycles_total, cycles_current, roundoff;
static int double_shift; /* JS doubleSpeedShifter: 1 in CGB double-speed mode */
static long instructions;
static int stop_emu;
static int failures;
static char last_state[512];

static double now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (double)ts.tv_sec * 1e3 + (double)ts.tv_nsec / 1e6;
}

/* ---- ROM image: the JS decoder, including its handling of "==" ---- */

static int b64_value(char c) {
  if (c >= 'A' && c <= 'Z') return c - 'A';
  if (c >= 'a' && c <= 'z') return c - 'a' + 26;
  if (c >= '0' && c <= '9') return c - '0' + 52;
  if (c == '+') return 62;
  if (c == '/') return 63;
  if (c == '=') return 64; /* indexOf() in the JS alphabet puts '=' at 64 */
  return -1;
}

/* base64_decode() in gbemu-part1.js takes the non-native path (no atob), which
 * emits three bytes per four characters with no trimming, so the final "==" group
 * yields two bytes beyond the 65536 of the cartridge (0x10 and 0x40). The expected
 * sumROM includes them. */
static uint8_t *decode_rom(const char *b64, size_t len, long *out_len) {
  size_t groups = len / 4;
  uint8_t *out = malloc(groups * 3 + 1);
  if (!out) {
    fprintf(stderr, "gbemu: out of memory\n");
    exit(1);
  }
  for (size_t g = 0; g < groups; g++) {
    int s0 = b64_value(b64[4 * g]);
    int s1 = b64_value(b64[4 * g + 1]);
    int s2 = b64_value(b64[4 * g + 2]);
    int s3 = b64_value(b64[4 * g + 3]);
    out[3 * g] = (uint8_t)((s0 << 2) | (s1 >> 4));
    out[3 * g + 1] = (uint8_t)(((s1 & 0x0F) << 4) | (s2 >> 2));
    out[3 * g + 2] = (uint8_t)(((s2 & 0x03) << 6) | s3);
  }
  *out_len = (long)groups * 3;
  return out;
}

/* ---- memory map ---- */

static void set_rom_base(void) {
  long edge = rom_len / 0x4000;
  switch (rom_offs) {
    case 0x00:
    case 0x20:
    case 0x40:
    case 0x60:
      rom_base = (long)(rom_offs % edge) * 0x4000;
      break;
    default:
      rom_base = (long)((rom_offs % edge) - 1) * 0x4000;
  }
}

static uint8_t io_read(uint16_t a) {
  switch (a) {
    case 0xFF00: return (uint8_t)((memory[a] & 0x30) | 0xCF);
    case 0xFF02: return (uint8_t)(memory[a] | 0x7E);
    case 0xFF07: return (uint8_t)(0xF8 | memory[a]);
    case 0xFF0F: return (uint8_t)(0xE0 | if_reg);
    case 0xFFFF: return (uint8_t)ie_reg;
    case 0xFF41: return (uint8_t)(0x80 | memory[a] | mode_stat);
    default: return memory[a];
  }
}

static uint8_t rd(uint16_t a) {
  long i;
  if (a < 0x4000) return rom[a];
  if (a < 0x8000) {
    i = rom_base + a;
    return (i >= 0 && i < rom_len) ? rom[i] : 0xFF;
  }
  if (a < 0xA000) {
    if (mode_stat > 2) return 0xFF;
    if (a >= 0x9800) return bgchr[vram_bank][a & 0x7FF];
    return vram_bank ? vram1[a & 0x1FFF] : memory[a];
  }
  if (a < 0xC000) {
    i = (long)a + ram_pos;
    return (ram_enabled && i >= 0 && i < 0x8000) ? mbc_ram[i] : 0xFF;
  }
  if (a < 0xE000) return memory[a];
  if (a < 0xFE00) return memory[a - 0x2000];
  if (a < 0xFEA0) return mode_stat > 1 ? 0xFF : memory[a];
  if (a < 0xFF00) return 0xFF;
  return io_read(a);
}

static void request_irq(uint8_t bit) { if_reg |= bit; }

/* checkIRQMatching: the latch is refreshed only here (and at the other JS check points). */
static void check_irq(void) {
  if (ime) irq_line = ie_reg & if_reg & 0x1F;
}

static void request_irq_check(uint8_t bit) {
  if_reg |= bit;
  check_irq();
}

/* LYC compare: FF41 bit 2 follows LY == LYC, and IF bit 1 is set when the LYC source is on. */
static void lcd_lyc_compare(int check) {
  if (memory[0xFF44] == memory[0xFF45]) {
    memory[0xFF41] |= 0x04;
    if (lyc_trig) {
      if (check) request_irq_check(0x02);
      else request_irq(0x02);
    }
  } else {
    memory[0xFF41] &= 0x7B;
  }
}

static void match_lyc(void) { lcd_lyc_compare(1); }


static void lcdc_write(uint8_t v) {
  if (memory[0xFF40] != v) {
    int on = v > 0x7F;
    if (on != lcd_on) {
      lcd_on = on;
      memory[0xFF41] &= 0x78;
      lcd_ticks = 0;
      stat_tracker = 0;
      actual_line = 0;
      memory[0xFF44] = 0;
      if (lcd_on) {
        mode_stat = 2;
        match_lyc();
      } else {
        mode_stat = 0;
      }
      if_reg &= 0xFD;
    }
    memory[0xFF40] = v;
  }
}

static void dma_transfer(uint8_t v) {
  uint16_t base = (uint16_t)(v << 8);
  int stat = mode_stat;
  memory[0xFF46] = v;
  if (v < 0xE0) { /* the CGB writer copies from every region below 0xE0 */
    mode_stat = 0;
    for (int i = 0; i < 160; i++) memory[0xFE00 + i] = rd((uint16_t)(base + i));
    mode_stat = stat;
  }
}

static void io_write(uint16_t a, uint8_t v) {
  if (a >= 0xFF10 && a <= 0xFF25 && !sound_on && a != 0xFF15 && a != 0xFF1F) return;
  switch (a) {
    case 0xFF04:
      memory[a] = 0;
      div_ticks &= 0xFF;
      break;
    case 0xFF07: {
      int sel = v & 3;
      memory[a] = v;
      tima_enabled = (v & 4) != 0;
      if (sel == 0) sel = 4;
      tac_clocker = (1L << (2 * sel)) << 2; /* Math.pow(4, sel) << 2 */
      break;
    }
    case 0xFF0F:
      if_reg = v;
      check_irq();
      break;
    case 0xFFFF:
      ie_reg = v;
      check_irq();
      break;
    case 0xFF26:
      /* NR52: power off clears the sound registers (through their writers, which ignore writes
       * while the APU is off in CGB mode; the two unused ones have no writer and take the zeros) */
      if (sound_on && v < 0x80) {
        memory[a] = 0;
        sound_on = 0;
        memory[0xFF15] = 0;
        memory[0xFF1F] = 0;
      } else if (!sound_on && v > 0x7F) {
        memory[a] = 0x80;
        sound_on = 1;
      }
      break;
    case 0xFF40:
      lcdc_write(v);
      break;
    case 0xFF41:
      lyc_trig = (v & 0x40) != 0;
      mode2_trig = (v & 0x20) != 0;
      mode1_trig = (v & 0x10) != 0;
      mode0_trig = (v & 0x08) != 0;
      memory[a] = (uint8_t)(v & 0x78);
      break;
    case 0xFF44: /* read-only, but a write restarts the LCD line counter while the LCD is on */
      if (lcd_on) {
        mode_stat = 2;
        lcd_ticks = 0;
        stat_tracker = 0;
        actual_line = 0;
        memory[0xFF44] = 0;
      }
      break;
    case 0xFF45:
      if (memory[a] != v) {
        memory[a] = v;
        if (lcd_on) match_lyc();
      }
      break;
    case 0xFF46:
      dma_transfer(v);
      break;
    case 0xFF4F:
      vram_bank = v & 0x01;
      break;
    case 0xFF68:
      memory[0xFF69] = bg_raw_palette[v & 0x3F];
      memory[a] = v;
      break;
    case 0xFF69:
      bg_raw_palette[memory[0xFF68] & 0x3F] = v;
      if (memory[0xFF68] > 0x7F) {
        int next = (memory[0xFF68] + 1) & 0x3F;
        memory[0xFF68] = (uint8_t)(next | 0x80);
        memory[0xFF69] = bg_raw_palette[next];
      } else {
        memory[0xFF69] = v;
      }
      break;
    case 0xFF6A:
      memory[0xFF6B] = obj_raw_palette[v & 0x3F];
      memory[a] = v;
      break;
    case 0xFF6B:
      obj_raw_palette[memory[0xFF6A] & 0x3F] = v;
      if (memory[0xFF6A] > 0x7F) {
        int next = (memory[0xFF6A] + 1) & 0x3F;
        memory[0xFF6A] = (uint8_t)(next | 0x80);
        memory[0xFF6B] = obj_raw_palette[next];
      } else {
        memory[0xFF6B] = v;
      }
      break;
    case 0xFF51:
      if (!hdma_running) memory[a] = v;
      break;
    case 0xFF52:
      if (!hdma_running) memory[a] = v & 0xF0;
      break;
    case 0xFF53:
      if (!hdma_running) memory[a] = v & 0x1F;
      break;
    case 0xFF54:
      if (!hdma_running) memory[a] = v & 0xF0;
      break;
    case 0xFF55:
      if (!hdma_running) {
        if ((v & 0x80) == 0) {
          dma_write((v & 0x7F) + 1); /* general-purpose DMA, completed at once */
          memory[a] = 0xFF;
        } else {
          hdma_running = 1;
          memory[a] = v & 0x7F;
        }
      } else if ((v & 0x80) == 0) {
        hdma_running = 0;
        memory[a] |= 0x80;
      } else {
        memory[a] = v & 0x7F;
      }
      break;
    case 0xFF4D:
      memory[a] = (uint8_t)((v & 0x7F) | (memory[a] & 0x80));
      break;
    default:
      memory[a] = v;
  }
}

static void wr(uint16_t a, uint8_t v) {
  long i;
  if (a < 0x2000) {
    ram_enabled = (v & 0x0F) == 0x0A;
    return;
  }
  if (a < 0x4000) {
    rom_offs = (rom_offs & 0x60) | (v & 0x1F);
    set_rom_base();
    return;
  }
  if (a < 0x6000) {
    if (mbc1_mode) {
      ram_pos = (long)((v & 3) << 13) - 0xA000;
    } else {
      rom_offs = ((v & 3) << 5) | (rom_offs & 0x1F);
      set_rom_base();
    }
    return;
  }
  if (a < 0x8000) {
    mbc1_mode = v & 1;
    if (mbc1_mode) {
      rom_offs &= 0x1F;
      set_rom_base();
    } else {
      ram_pos = -0xA000;
    }
    return;
  }
  if (a < 0xA000) {
    if (mode_stat < 3) {
      if (a >= 0x9800) bgchr[vram_bank][a & 0x7FF] = v;
      else if (vram_bank) vram1[a & 0x1FFF] = v;
      else memory[a] = v;
    }
    return;
  }
  if (a < 0xC000) {
    i = (long)a + ram_pos;
    if (ram_enabled && i >= 0 && i < 0x8000) mbc_ram[i] = v;
    return;
  }
  if (a < 0xE000) {
    memory[a] = v;
    return;
  }
  if (a < 0xFE00) {
    memory[a - 0x2000] = v;
    return;
  }
  if (a < 0xFEA0) {
    if (mode_stat < 2) memory[a] = v;
    return;
  }
  if (a < 0xFF00) return;
  io_write(a, v);
}

static void render_line(int ly) {
  static const uint32_t shade_colour[4] = {0xF8F8F8, 0xA8A8A8, 0x505050, 0x000000};
  uint8_t lcdc = memory[0xFF40];
  int scy = memory[0xFF42], scx = memory[0xFF43];
  uint8_t bgp = memory[0xFF47];
  int map_base = (lcdc & 0x08) ? 0x9C00 : 0x9800;
  int signed_tiles = !(lcdc & 0x10);
  int y = (scy + ly) & 0xFF;
  for (int x = 0; x < 160; x++) {
    int px = (scx + x) & 0xFF;
    int shade = 0;
    if (lcdc & 0x01) {
      int idx = memory[map_base + (y >> 3) * 32 + (px >> 3)];
      int tile;
      if (signed_tiles) {
        int sidx = idx < 128 ? idx : idx - 256;
        tile = 0x9000 + sidx * 16;
      } else {
        tile = 0x8000 + idx * 16;
      }
      int row = (y & 7) * 2;
      int bit = 7 - (px & 7);
      int lo = (memory[tile + row] >> bit) & 1;
      int hi = (memory[tile + row + 1] >> bit) & 1;
      shade = (bgp >> (((hi << 1) | lo) * 2)) & 3;
    }
    frame[ly * 160 + x] = shade_colour[shade];
  }
}

/* DMAWrite (CGB VRAM DMA): copies `tiles` 16-byte blocks from FF51/FF52 to FF53/FF54 and clocks
 * the CPU for them. Destinations at 0x9800 and above go to the tile-map arrays (bgchr). */
static void dma_write(int tiles) {
  uint16_t source;
  int destination;
  if (!halted) cpu_ticks += 4 | ((tiles << 5) << double_shift);
  source = (uint16_t)((memory[0xFF51] << 8) | memory[0xFF52]);
  destination = (memory[0xFF53] << 8) | memory[0xFF54];
  do {
    if (destination < 0x1800) {
      for (int k = 0; k < 16; k++) {
        uint8_t val = rd(source++);
        if (vram_bank) vram1[destination + k] = val;
        else memory[0x8000 + destination + k] = val;
      }
      destination += 0x10;
    } else {
      destination &= 0x7F0;
      for (int k = 0; k < 16; k++) bgchr[vram_bank][destination++] = rd(source++);
      destination = (destination + 0x1800) & 0x1FF0;
    }
    source &= 0xFFF0;
    --tiles;
  } while (tiles > 0);
  /* the JS writes the advanced addresses back to the HDMA registers */
  memory[0xFF51] = (uint8_t)(source >> 8);
  memory[0xFF52] = (uint8_t)(source & 0xF0);
  memory[0xFF53] = (uint8_t)(destination >> 8);
  memory[0xFF54] = (uint8_t)(destination & 0xF0);
}

/* executeHDMA: one H-blank block, run from the mode 0 and line-end points while HDMA is active. */
static void execute_hdma(void) {
  dma_write(1);
  if (halted) {
    if ((lcd_ticks - sprite_count) < ((4 >> double_shift) | 0x20)) {
      cpu_ticks = 4 + ((0x20 + sprite_count) << double_shift);
      lcd_ticks = sprite_count + ((4 >> double_shift) | 0x20);
    }
  } else {
    lcd_ticks += (4 >> double_shift) | 0x20;
  }
  if (memory[0xFF55] == 0) {
    hdma_running = 0;
    memory[0xFF55] = 0xFF;
  } else {
    --memory[0xFF55];
  }
}

/* ---- LCD: the JS LINECONTROL state machine. The handler is picked from actual_line; LY is
 * memory[0xFF44], advanced with ++ as the JS does. The STAT mode, trigger flags and IF bits follow
 * the JS handlers, including the sites where the JS sets IF without re-checking the IRQ latch.
 * Pixels come from render_line (background only); sprites enter through sprite_count, which sets
 * the mode 3 length. ---- */

static void line_control(void);

static void lcd_render_line(void) {
  if (actual_line < 144) render_line(actual_line);
}

/* updateSpriteCount: in CGB mode each sprite on the line adds 6 dots to mode 3. */
static void update_sprite_count(int line) {
  sprite_count = 252;
  if (memory[0xFF40] & 0x02) {
    int line_adjusted = line + 0x10;
    int y_cap = (memory[0xFF40] & 0x04) ? 0x10 : 0x08;
    for (int oam = 0xFE00; oam < 0xFEA0 && sprite_count < 312; oam += 4) {
      int yoffset = line_adjusted - memory[oam];
      if (yoffset > -1 && yoffset < y_cap) sprite_count += 6;
    }
  }
}

static void lcd_mode2(void) {
  if (stat_tracker != 1) {
    if (mode2_trig) request_irq_check(0x02);
    stat_tracker = 1;
    mode_stat = 2;
  }
}

static void lcd_mode3(void) {
  if (mode_stat != 3) {
    if (stat_tracker == 0 && mode2_trig) request_irq_check(0x02);
    stat_tracker = 1;
    mode_stat = 3;
  }
}

static void lcd_mode0(void) {
  if (mode_stat != 0) {
    if (stat_tracker != 2) {
      if (stat_tracker == 0) {
        if (mode2_trig) request_irq_check(0x02);
        mode_stat = 3;
      }
      lcd_render_line();
      update_sprite_count(actual_line);
      stat_tracker = 2;
    }
    if (lcd_ticks >= sprite_count) {
      if (hdma_running) execute_hdma();
      if (mode0_trig) request_irq_check(0x02);
      stat_tracker = 3;
      mode_stat = 0;
    }
  }
}

/* The "new scan line" block of the visible lines, run before LY advances. The JS sets IF here
 * without re-checking the latch. */
static void lcd_end_of_line(void) {
  if (stat_tracker != 3) {
    if (stat_tracker != 2) {
      if (stat_tracker == 0 && mode2_trig) request_irq(0x02);
      lcd_render_line();
    }
    if (hdma_running) execute_hdma();
    if (mode0_trig) request_irq(0x02);
  }
}

/* Lines 0..142 (last = 0) and line 143 (last = 1, which enters VBlank). */
static void lcd_visible_line(int last) {
  if (lcd_ticks < 80) {
    lcd_mode2();
  } else if (lcd_ticks < 252) {
    lcd_mode3();
  } else if (lcd_ticks < 456) {
    lcd_mode0();
  } else {
    lcd_ticks -= 456;
    if (stat_tracker != 3) lcd_end_of_line();
    if (last) {
      actual_line = memory[0xFF44] = 144;
      lcd_lyc_compare(0);
      stat_tracker = 0;
      mode_stat = 1;
      if_reg |= (mode1_trig ? 0x03 : 0x01);
      check_irq();
    } else {
      actual_line = ++memory[0xFF44];
      lcd_lyc_compare(0);
      check_irq();
      stat_tracker = 0;
      mode_stat = 2;
    }
    line_control();
  }
}

/* Lines 144..152. */
static void lcd_vblank_line(void) {
  if (lcd_ticks >= 456) {
    lcd_ticks -= 456;
    actual_line = ++memory[0xFF44];
    lcd_lyc_compare(1);
    line_control();
  }
}

/* Line 153: LY reads 0 early, then the frame restarts at line 0. */
static void lcd_last_line(void) {
  if (lcd_ticks >= 8) {
    if (stat_tracker != 4 && memory[0xFF44] == 153) {
      memory[0xFF44] = 0;
      lcd_lyc_compare(1);
      stat_tracker = 4;
    }
    if (lcd_ticks >= 456) {
      lcd_ticks -= 456;
      stat_tracker = 0;
      actual_line = 0;
      line_control();
    }
  }
}

static void line_control(void) {
  if (actual_line < 143) {
    lcd_visible_line(0);
  } else if (actual_line == 143) {
    lcd_visible_line(1);
  } else if (actual_line < 153) {
    lcd_vblank_line();
  } else {
    lcd_last_line();
  }
}

static void lcd_step(long ticks) {
  lcd_ticks += ticks;
  if (lcd_on) line_control();
}

/* ---- clocking, timers and interrupts ---- */

static void update_core(void) {
  long t;
  lcd_step(cpu_ticks >> double_shift); /* HDMA inside the LCD step can add to cpu_ticks */
  t = cpu_ticks;
  emulator_ticks += (double)(t >> double_shift);
  div_ticks += t;
  if (tima_enabled) {
    timer_ticks += t;
    while (timer_ticks >= tac_clocker) {
      int tima;
      timer_ticks -= tac_clocker;
      tima = memory[0xFF05] + 1;
      memory[0xFF05] = (uint8_t)tima;
      if (tima == 0x100) {
        memory[0xFF05] = memory[0xFF06];
        request_irq_check(0x04);
      }
    }
  }
}

static void recalc_cycles(void) {
  double end_mod = cycles_current - 4.0 * (double)(long)(cycles_current / 4.0);
  cycles_total = (double)CYCLES_BASE + cycles_current - end_mod;
  cycles_current = end_mod;
}

static void iteration_end(void) {
  if ((stop_emu & 1) == 0) {
    memory[0xFF04] = (uint8_t)((memory[0xFF04] + (div_ticks >> 8)) & 0xFF);
    div_ticks &= 0xFF;
    stop_emu |= 1;
    emulator_ticks -= cycles_total;
    cycles_current += roundoff;
    recalc_cycles();
  }
}

static void push16(uint16_t v) {
  sp = (uint16_t)(sp - 1);
  wr(sp, (uint8_t)(v >> 8));
  sp = (uint16_t)(sp - 1);
  wr(sp, (uint8_t)(v & 0xFF));
}

static uint16_t pop16(void) {
  uint8_t lo = rd(sp);
  sp = (uint16_t)(sp + 1);
  uint8_t hi = rd(sp);
  sp = (uint16_t)(sp + 1);
  return (uint16_t)((hi << 8) | lo);
}

/* launchIRQ: the lowest bit of the latch fires. The JS subtracts the bit from IF and clears the
 * whole latch, so a stale latch can fire after IF has changed. */
static void launch_irq(void) {
  for (int b = 0; b < 5; b++) {
    int testbit = 1 << b;
    if ((testbit & irq_line) == testbit) {
      ime = 0;
      if_reg -= testbit;
      irq_line = 0;
      cpu_ticks = 20;
      push16(pc);
      pc = (uint16_t)(0x40 | (b << 3));
      update_core();
      return;
    }
  }
}

/* ---- registers ---- */

static uint16_t get_bc(void) { return (uint16_t)((reg_b << 8) | reg_c); }
static uint16_t get_de(void) { return (uint16_t)((reg_d << 8) | reg_e); }
static uint16_t get_hl(void) { return (uint16_t)((reg_h << 8) | reg_l); }
static uint16_t get_af(void) { return (uint16_t)((reg_a << 8) | (reg_f & 0xF0)); }
static void set_bc(uint16_t v) { reg_b = (uint8_t)(v >> 8); reg_c = (uint8_t)v; }
static void set_de(uint16_t v) { reg_d = (uint8_t)(v >> 8); reg_e = (uint8_t)v; }
static void set_hl(uint16_t v) { reg_h = (uint8_t)(v >> 8); reg_l = (uint8_t)v; }
static void set_af(uint16_t v) { reg_a = (uint8_t)(v >> 8); reg_f = (uint8_t)(v & 0xF0); }

/* rp: BC DE HL SP; rp2: BC DE HL AF */
static uint16_t get_rp(int i) {
  switch (i) {
    case 0: return get_bc();
    case 1: return get_de();
    case 2: return get_hl();
    default: return sp;
  }
}
static void set_rp(int i, uint16_t v) {
  switch (i) {
    case 0: set_bc(v); break;
    case 1: set_de(v); break;
    case 2: set_hl(v); break;
    default: sp = v; break;
  }
}
static uint16_t get_rp2(int i) { return i == 3 ? get_af() : get_rp(i); }
static void set_rp2(int i, uint16_t v) {
  if (i == 3) set_af(v);
  else set_rp(i, v);
}

/* r: B C D E H L (HL) A */
static uint8_t get_r(int r) {
  switch (r) {
    case 0: return reg_b;
    case 1: return reg_c;
    case 2: return reg_d;
    case 3: return reg_e;
    case 4: return reg_h;
    case 5: return reg_l;
    case 7: return reg_a;
    default: return 0;
  }
}
static void set_r(int r, uint8_t v) {
  switch (r) {
    case 0: reg_b = v; break;
    case 1: reg_c = v; break;
    case 2: reg_d = v; break;
    case 3: reg_e = v; break;
    case 4: reg_h = v; break;
    case 5: reg_l = v; break;
    case 7: reg_a = v; break;
    default: break;
  }
}
static uint8_t rd_r(int r) { return r == 6 ? rd(get_hl()) : get_r(r); }
static void wr_r(int r, uint8_t v) {
  if (r == 6) wr(get_hl(), v);
  else set_r(r, v);
}

static uint8_t fetch8(void) {
  uint8_t v = rd(pc);
  pc = (uint16_t)(pc + 1);
  return v;
}
static int fetch_s8(void) {
  int v = fetch8();
  return v < 128 ? v : v - 256;
}
static uint16_t fetch16(void) {
  uint8_t lo = fetch8();
  uint8_t hi = fetch8();
  return (uint16_t)((hi << 8) | lo);
}

static int cond(int cc) {
  switch (cc) {
    case 0: return !(reg_f & FZ);
    case 1: return (reg_f & FZ) != 0;
    case 2: return !(reg_f & FC);
    default: return (reg_f & FC) != 0;
  }
}

/* ---- arithmetic and logic ---- */

static void alu(int op, uint8_t v) {
  unsigned a = reg_a;
  unsigned c = (reg_f & FC) ? 1u : 0u;
  unsigned r;
  switch (op) {
    case 0:
      r = a + v;
      reg_f = (uint8_t)(((r & 0xFF) == 0 ? FZ : 0) | ((a & 0xF) + (v & 0xF) > 0xF ? FH : 0) |
                        (r > 0xFF ? FC : 0));
      reg_a = (uint8_t)r;
      break;
    case 1:
      r = a + v + c;
      reg_f = (uint8_t)(((r & 0xFF) == 0 ? FZ : 0) | ((a & 0xF) + (v & 0xF) + c > 0xF ? FH : 0) |
                        (r > 0xFF ? FC : 0));
      reg_a = (uint8_t)r;
      break;
    case 2:
    case 7:
      r = a - v;
      reg_f = (uint8_t)(((r & 0xFF) == 0 ? FZ : 0) | FN | ((a & 0xF) < (v & 0xF) ? FH : 0) |
                        (a < v ? FC : 0));
      if (op == 2) reg_a = (uint8_t)r;
      break;
    case 3:
      r = a - v - c;
      reg_f = (uint8_t)(((r & 0xFF) == 0 ? FZ : 0) | FN | ((a & 0xF) < (v & 0xF) + c ? FH : 0) |
                        (a < v + c ? FC : 0));
      reg_a = (uint8_t)r;
      break;
    case 4:
      reg_a &= v;
      reg_f = (uint8_t)((reg_a == 0 ? FZ : 0) | FH);
      break;
    case 5:
      reg_a ^= v;
      reg_f = (uint8_t)(reg_a == 0 ? FZ : 0);
      break;
    default:
      reg_a |= v;
      reg_f = (uint8_t)(reg_a == 0 ? FZ : 0);
      break;
  }
}

static uint8_t inc8(uint8_t v) {
  uint8_t r = (uint8_t)(v + 1);
  reg_f = (uint8_t)((reg_f & FC) | (r == 0 ? FZ : 0) | ((v & 0xF) == 0xF ? FH : 0));
  return r;
}

static uint8_t dec8(uint8_t v) {
  uint8_t r = (uint8_t)(v - 1);
  reg_f = (uint8_t)((reg_f & FC) | (r == 0 ? FZ : 0) | FN | ((v & 0xF) == 0 ? FH : 0));
  return r;
}

static void add_hl(uint16_t v) {
  unsigned hl = get_hl();
  unsigned r = hl + v;
  reg_f = (uint8_t)((reg_f & FZ) | ((hl & 0xFFF) + (v & 0xFFF) > 0xFFF ? FH : 0) |
                    (r > 0xFFFF ? FC : 0));
  set_hl((uint16_t)r);
}

/* SP + e8: the flags come from the low byte, Z and N are cleared. */
static uint16_t sp_plus(uint8_t eu) {
  int e = eu < 128 ? eu : eu - 256;
  reg_f = (uint8_t)((((sp & 0xF) + (eu & 0xF)) > 0xF ? FH : 0) |
                    (((sp & 0xFF) + eu) > 0xFF ? FC : 0));
  return (uint16_t)((sp + e) & 0xFFFF);
}

/* Rotates and shifts: 0 RLC 1 RRC 2 RL 3 RR 4 SLA 5 SRA 6 SWAP 7 SRL. */
static uint8_t shift_op(int op, uint8_t v) {
  uint8_t r;
  int c;
  int old_c = (reg_f & FC) ? 1 : 0;
  switch (op) {
    case 0: c = v >> 7; r = (uint8_t)((v << 1) | c); break;
    case 1: c = v & 1; r = (uint8_t)((v >> 1) | (c << 7)); break;
    case 2: c = v >> 7; r = (uint8_t)((v << 1) | old_c); break;
    case 3: c = v & 1; r = (uint8_t)((v >> 1) | (old_c << 7)); break;
    case 4: c = v >> 7; r = (uint8_t)(v << 1); break;
    case 5: c = v & 1; r = (uint8_t)((v >> 1) | (v & 0x80)); break;
    case 6: c = 0; r = (uint8_t)((v << 4) | (v >> 4)); break;
    default: c = v & 1; r = (uint8_t)(v >> 1); break;
  }
  reg_f = (uint8_t)((r == 0 ? FZ : 0) | (c ? FC : 0));
  return r;
}

static void daa(void) {
  int a = reg_a;
  int c = (reg_f & FC) != 0;
  int h = (reg_f & FH) != 0;
  int n = (reg_f & FN) != 0;
  if (!n) {
    if (c || a > 0x99) {
      a += 0x60;
      c = 1;
    }
    if (h || (a & 0x0F) > 0x09) a += 0x06;
  } else {
    if (c) a -= 0x60;
    if (h) a -= 0x06;
  }
  reg_a = (uint8_t)a;
  reg_f = (uint8_t)((reg_a == 0 ? FZ : 0) | (n ? FN : 0) | (c ? FC : 0));
}

/* HALT: with an IRQ already matched the CGB core adds 4 ticks (the DMG HALT bug is not used by
 * CGB cartridges); otherwise the CPU halts. */
static void do_halt(void) {
  if (ie_reg & if_reg & 0x1F) {
    cpu_ticks += 4;
  } else {
    halted = 1;
  }
}

/* ---- instructions ---- */

static void exec_cb(void) {
  uint8_t op = fetch8();
  int z = op & 7;
  int y = (op >> 3) & 7;
  uint8_t v;
  cpu_ticks = TICK_TABLE_CB[op];
  v = rd_r(z);
  if (op < 0x40) {
    wr_r(z, shift_op(y, v));
  } else if (op < 0x80) {
    reg_f = (uint8_t)((reg_f & FC) | FH | (((v >> y) & 1) ? 0 : FZ));
  } else if (op < 0xC0) {
    wr_r(z, (uint8_t)(v & ~(1u << y)));
  } else {
    wr_r(z, (uint8_t)(v | (1u << y)));
  }
}

/* STOP is one byte in the JS core. With KEY1 bit 0 set, the CGB switches speed (the JS does the
 * same: the CPU keeps running and the LCD and audio clocks use the shifted tick count). Without a
 * request the JS halts the CPU until joypad input, which this run never provides; the cartridge is
 * checked not to reach that path (failures is raised if it does). */
static void stop_op(void) {
  if (memory[0xFF4D] & 0x01) {
    if (memory[0xFF4D] > 0x7F) {
      double_shift = 0;
      memory[0xFF4D] &= 0x7F;
    } else {
      double_shift = 1;
      memory[0xFF4D] |= 0x80;
    }
    memory[0xFF4D] &= 0xFE;
  } else {
    failures++;
  }
}

static void exec_op(uint8_t op) {
  int x = op >> 6;
  int y = (op >> 3) & 7;
  int z = op & 7;
  uint16_t a;
  switch (x) {
    case 0:
      switch (z) {
        case 0:
          switch (y) {
            case 0: break;
            case 1:
              a = fetch16();
              wr(a, (uint8_t)(sp & 0xFF));
              wr((uint16_t)(a + 1), (uint8_t)(sp >> 8));
              break;
            case 2: stop_op(); break;
            case 3: {
              int e = fetch_s8();
              pc = (uint16_t)(pc + e);
              break;
            }
            default: {
              int e = fetch_s8();
              if (cond(y - 4)) {
                pc = (uint16_t)(pc + e);
                cpu_ticks += 4;
              }
              break;
            }
          }
          break;
        case 1:
          if (y & 1) {
            add_hl(get_rp(y >> 1));
          } else {
            set_rp(y >> 1, fetch16());
          }
          break;
        case 2:
          switch (y) {
            case 0: wr(get_bc(), reg_a); break;
            case 1: reg_a = rd(get_bc()); break;
            case 2: wr(get_de(), reg_a); break;
            case 3: reg_a = rd(get_de()); break;
            case 4: wr(get_hl(), reg_a); set_hl((uint16_t)(get_hl() + 1)); break;
            case 5: reg_a = rd(get_hl()); set_hl((uint16_t)(get_hl() + 1)); break;
            case 6: wr(get_hl(), reg_a); set_hl((uint16_t)(get_hl() - 1)); break;
            default: reg_a = rd(get_hl()); set_hl((uint16_t)(get_hl() - 1)); break;
          }
          break;
        case 3:
          set_rp(y >> 1, (uint16_t)(get_rp(y >> 1) + ((y & 1) ? -1 : 1)));
          break;
        case 4: wr_r(y, inc8(rd_r(y))); break;
        case 5: wr_r(y, dec8(rd_r(y))); break;
        case 6: wr_r(y, fetch8()); break;
        default:
          switch (y) {
            case 0: case 1: case 2: case 3: {
              uint8_t r = shift_op(y, reg_a);
              reg_f = (uint8_t)(reg_f & ~FZ);
              reg_a = r;
              break;
            }
            case 4: daa(); break;
            case 5: reg_a = (uint8_t)~reg_a; reg_f |= (FN | FH); break;
            case 6: reg_f = (uint8_t)((reg_f & FZ) | FC); break;
            default: reg_f = (uint8_t)((reg_f & FZ) | ((reg_f & FC) ? 0 : FC)); break;
          }
          break;
      }
      break;
    case 1:
      if (op == 0x76) do_halt();
      else wr_r(y, rd_r(z));
      break;
    case 2:
      alu(y, rd_r(z));
      break;
    default:
      switch (z) {
        case 0:
          switch (y) {
            case 0: case 1: case 2: case 3:
              if (cond(y)) {
                pc = pop16();
                cpu_ticks += 12;
              }
              break;
            case 4: a = fetch8(); wr((uint16_t)(0xFF00 | (uint8_t)a), reg_a); break;
            case 5: sp = sp_plus(fetch8()); break;
            case 6: a = fetch8(); reg_a = rd((uint16_t)(0xFF00 | (uint8_t)a)); break;
            default: set_hl(sp_plus(fetch8())); break;
          }
          break;
        case 1:
          if (y & 1) {
            switch (y) {
              case 1: pc = pop16(); break;
              case 3: pc = pop16(); ei_delay = (ei_delay == 2 || rd(pc) == 0x76) ? 1 : 2; break;
              case 5: pc = get_hl(); break;
              default: sp = get_hl(); break;
            }
          } else {
            set_rp2(y >> 1, pop16());
          }
          break;
        case 2:
          switch (y) {
            case 0: case 1: case 2: case 3:
              a = fetch16();
              if (cond(y)) {
                pc = a;
                cpu_ticks += 4;
              }
              break;
            case 4: wr((uint16_t)(0xFF00 | reg_c), reg_a); break;
            case 5: a = fetch16(); wr(a, reg_a); break;
            case 6: reg_a = rd((uint16_t)(0xFF00 | reg_c)); break;
            default: a = fetch16(); reg_a = rd(a); break;
          }
          break;
        case 3:
          switch (y) {
            case 0: pc = fetch16(); break;
            case 1: exec_cb(); break;
            case 6: ime = 0; ei_delay = 0; break;
            case 7: ei_delay = (ei_delay == 2 || rd(pc) == 0x76) ? 1 : 2; break;
            default: break; /* illegal opcodes act as NOPs */
          }
          break;
        case 4:
          if (y < 4) {
            a = fetch16();
            if (cond(y)) {
              push16(pc);
              pc = a;
              cpu_ticks += 12;
            }
          }
          break;
        case 5:
          if (y & 1) {
            if (y == 1) {
              a = fetch16();
              push16(pc);
              pc = a;
            }
          } else {
            push16(get_rp2(y >> 1));
          }
          break;
        case 6:
          alu(y, fetch8());
          break;
        default:
          push16(pc);
          pc = (uint16_t)(y * 8);
          break;
      }
      break;
  }
}

/* ---- run loop: the structure of GameBoyCore.run() and executeIteration() ---- */

static void check_final_state(void) {
  long sum_rom = 0, sum_mem = 0, sum_ram = 0;
  for (long i = 0; i < rom_len; i++) sum_rom += rom[i];
  for (long i = 0; i < 0x10000; i++) sum_mem += memory[i];
  for (long i = 0; i < 0x8000; i++) sum_ram += mbc_ram[i];
  snprintf(last_state, sizeof last_state,
           "{\"registerA\":%d,\"registerB\":%d,\"registerC\":%d,\"registerE\":%d,"
           "\"registersHL\":%d,\"programCounter\":%d,\"stackPointer\":%d,"
           "\"sumROM\":%ld,\"sumMemory\":%ld,\"sumMBCRam\":%ld,\"sumVRam\":0}",
           reg_a, reg_b, reg_c, reg_e, get_hl(), pc, sp, sum_rom, sum_mem, sum_ram);
  if (strcmp(last_state, EXPECTED_STATE) != 0) {
    fprintf(stderr, "gbemu: incorrect final state\n actual   %s\n expected %s\n", last_state,
            EXPECTED_STATE);
    failures++;
  }
}

static void execute_iteration(void) {
  while (stop_emu == 0) {
    int executed = 0;
    if (ei_delay == 1) {
      ime = 1;
      check_irq();
      ei_delay = 0;
    } else if (ei_delay == 2) {
      ei_delay = 1;
    }
    if (irq_line > 0) {
      halted = 0;
      launch_irq();
    }
    if (halted) {
      cpu_ticks = 4;
      update_core();
    } else {
      uint8_t op = rd(pc);
      pc = (uint16_t)(pc + 1);
      if (skip_pc_inc) {
        pc = (uint16_t)(pc - 1);
        skip_pc_inc = 0;
      }
      cpu_ticks = TICK_TABLE[op];
      exec_op(op);
      update_core();
      executed = 1;
    }
    if (emulator_ticks >= cycles_total) iteration_end();
    if (executed) {
      instructions++;
      if (instructions > TOTAL_INSTRUCTIONS) {
        iteration_end();
        stop_emu |= 2;
        check_final_state();
      }
    }
  }
}

static void run_once(void) {
  if ((stop_emu & 2) == 0 && (stop_emu & 1) == 1) {
    stop_emu = 0;
    execute_iteration();
  }
}

/* start(): post-boot state (initSkipBootstrap) and the cartridge (ROMLoad). */
static void init_state(void) {
  memset(memory, 0, sizeof memory);
  memcpy(memory, rom, 0x4000);
  memset(mbc_ram, 0, sizeof mbc_ram);
  for (int i = 0; i < 256; i++) memory[0xFF00 + i] = FFXX_DUMP[i];
  memory[0xFF0F] = 0; /* JS: IF and IE live outside memory[] */
  memory[0xFFFF] = 0;
  if_reg = 0xE1;
  ie_reg = 0;
  sound_on = 1;
  memory[0xFF74] = 0xFE;
  double_shift = 0;
  hdma_running = 0;
  vram_bank = 0;
  memset(vram1, 0, sizeof vram1);
  memset(bgchr, 0, sizeof bgchr);
  memset(bg_raw_palette, 0, sizeof bg_raw_palette);
  memset(obj_raw_palette, 0, sizeof obj_raw_palette);
  reg_a = 0x11; /* CGB boot value: the cartridge (0x143 = 0x80) runs in CGB mode */
  reg_f = 0xB0;
  reg_b = 0x00;
  reg_c = 0x13;
  reg_d = 0x00;
  reg_e = 0xD8;
  reg_h = 0x01;
  reg_l = 0x4D;
  pc = 0x0100;
  sp = 0xFFFE;
  ime = 0;
  irq_line = 0;
  ei_delay = 0;
  halted = 0;
  skip_pc_inc = 0;
  cpu_ticks = 0;
  rom_offs = 0;
  rom_base = 0;
  ram_pos = -0xA000;
  ram_enabled = 0;
  mbc1_mode = 0;
  actual_line = 144;
  mode_stat = 1;
  lcd_ticks = 160;
  stat_tracker = 0;
  sprite_count = 252;
  mode0_trig = mode1_trig = mode2_trig = lyc_trig = 0;
  lcd_on = 1;
  div_ticks = 27044;
  timer_ticks = 0;
  tima_enabled = 0;
  tac_clocker = 1024;
  emulator_ticks = 0;
  cycles_total = (double)CYCLES_BASE;
  cycles_current = 0;
  roundoff = 1.216; /* 16777.216 - 16776 */
  instructions = 0;
  stop_emu = 1;
}

static void run_iteration(void) {
  init_state();
  while (instructions <= TOTAL_INSTRUCTIONS) run_once();
}

int main(void) {
  double t0, ms;
  rom = decode_rom(ROM_B64, sizeof ROM_B64 - 1, &rom_len);
  run_iteration(); /* warm-up */
  t0 = now_ms();
  for (int it = 0; it < ITERATIONS; it++) run_iteration();
  ms = now_ms() - t0;
  printf("gbemu %d iterations %.3f ms\n", ITERATIONS, ms);
  if (failures) {
    printf("validation FAILED: %d iterations ended in a different final state\n", failures);
    printf("state %s\n", last_state);
  } else {
    printf("validation OK: final CPU and memory state matches the JS expectation\n");
  }
  free(rom);
  return failures ? 1 : 0;
}
