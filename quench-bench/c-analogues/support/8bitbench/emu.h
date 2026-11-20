/*
 * C port of the 8bitbench Rust emulator (rust/src/{cpu,ppu,memory,mapper_0,
 * console,controller,binloader,benchmark}.rs). Mirrors benchmark.rs:
 * emu_load_rom() is load_rom(), emu_tick() is bench_tick() with the frame
 * copy that is passed to update_video.
 */
#ifndef EMU_BENCH_EMU_H
#define EMU_BENCH_EMU_H

#include <stddef.h>
#include <stdint.h>

#define EMU_FRAME_WIDTH 256
#define EMU_FRAME_HEIGHT 240
#define EMU_FRAME_BYTES (EMU_FRAME_WIDTH * EMU_FRAME_HEIGHT * 4)

typedef struct Emu Emu;

/* Equivalent of benchmark::load_rom(file). Returns NULL on a malformed file. */
Emu *emu_load_rom(const uint8_t *file, size_t len);

/* Frees an emulator returned by emu_load_rom. Accepts NULL. */
void emu_free(Emu *emu);

/* Equivalent of benchmark::bench_tick(). Runs one frame with the given
 * controller string and returns the RGBA frame (EMU_FRAME_BYTES long),
 * valid until the next call. */
const uint8_t *emu_tick(Emu *emu, const char *input);

#endif
