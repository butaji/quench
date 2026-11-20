/*
 * C port of the 8bitbench Rust emulator. Source correspondence:
 *   cpu.rs         -> Cpu, addressing modes (mode_*), ALU ops (op_*), manual(), cpu_tick()
 *   ppu.rs         -> Ppu, ppu_read/ppu_write, ppu_tick(), draw_tile/draw_with_state,
 *                     prepare_draw(), ppudma()
 *   memory.rs      -> mirror_addr(), memory_read/memory_write, the Mapper interface
 *   mapper_0.rs    -> Mapper (mapper 0 only), including the deep copy that
 *                     dyn_clone performs for every mid-frame state and every draw
 *   console.rs     -> Chipset (chip_read/chip_write), console_tick(), prepare_draw
 *   controller.rs  -> Controller
 *   binloader.rs   -> emu_load_rom() header parsing
 *   benchmark.rs   -> emu_load_rom() (load_rom), emu_tick() (bench_tick)
 * The opcode table and palette are generated from cpu.rs and ppu.rs, so
 * they match the Rust sources byte for byte (including the Rust table's
 * own opcode assignments, e.g. CMP immediate at 0x69).
 */

#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "emu.h"

#define VBL 21u
#define PPU_WIDTH 256
#define PPU_HEIGHT 240

static const uint8_t PALETTE[192] = {
    124, 124, 124, 0, 0, 252, 0, 0, 188, 68, 40, 188,
    148, 0, 132, 168, 0, 32, 168, 16, 0, 136, 20, 0,
    80, 48, 0, 0, 120, 0, 0, 104, 0, 0, 88, 0,
    0, 64, 88, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    188, 188, 188, 0, 120, 248, 0, 88, 248, 104, 68, 252,
    216, 0, 204, 228, 0, 88, 248, 56, 0, 228, 92, 16,
    172, 124, 0, 0, 184, 0, 0, 168, 0, 0, 168, 68,
    0, 136, 136, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    248, 248, 248, 60, 188, 252, 104, 136, 252, 152, 120, 248,
    248, 120, 248, 248, 88, 152, 248, 120, 88, 252, 160, 68,
    248, 184, 0, 184, 248, 24, 88, 216, 84, 88, 248, 152,
    0, 232, 216, 120, 120, 120, 0, 0, 0, 0, 0, 0,
    252, 252, 252, 164, 228, 252, 184, 184, 248, 216, 184, 248,
    248, 184, 248, 248, 164, 192, 240, 208, 176, 252, 224, 168,
    248, 216, 120, 216, 248, 120, 184, 248, 184, 184, 248, 216,
    0, 252, 252, 248, 216, 248, 0, 0, 0, 0, 0, 0,
};

static void fail(const char *msg) __attribute__((noreturn));
static void fail(const char *msg) {
    fprintf(stderr, "8bitbench emu: %s\n", msg);
    abort();
}

/* Rust slice/array indexing is bounds checked; mirror that. */
static size_t chk(size_t i, size_t n) {
    if (i >= n)
        fail("index out of range");
    return i;
}

/* memory.rs mirror_addr(from, to, addr): the `to` range only supplies its start. */
static uint16_t mirror_addr(uint16_t from_start, uint16_t from_end, uint16_t to_start, uint16_t addr) {
    uint16_t size = (uint16_t)(from_end - from_start + 1);
    uint16_t offset = (uint16_t)((uint16_t)(addr - to_start) % size);
    return (uint16_t)(from_start + offset);
}

/* ------------------------------------------------------------------ Mapper */

typedef struct {
    uint8_t *prg;
    size_t prg_len;
    uint8_t *prg_ram;
    size_t prg_ram_len;
    uint8_t *chr;
    size_t chr_len;
} Mapper;

static uint8_t *dup_bytes(const uint8_t *src, size_t n) {
    uint8_t *p = malloc(n ? n : 1);
    if (!p)
        fail("out of memory");
    if (n)
        memcpy(p, src, n);
    return p;
}

/* Mapper0::new: takes ownership of copies of prg and chr, zeroed PRG RAM. */
static void mapper_init(Mapper *m, const uint8_t *prg, size_t prg_len, size_t prg_ram_len,
                        const uint8_t *chr, size_t chr_len) {
    m->prg = dup_bytes(prg, prg_len);
    m->prg_len = prg_len;
    m->prg_ram = calloc(prg_ram_len ? prg_ram_len : 1, 1);
    if (!m->prg_ram)
        fail("out of memory");
    m->prg_ram_len = prg_ram_len;
    m->chr = dup_bytes(chr, chr_len);
    m->chr_len = chr_len;
}

/* dyn_clone::clone_box on a Mapper0: deep copy of all three Vecs. */
static void mapper_copy(Mapper *dst, const Mapper *src) {
    dst->prg = dup_bytes(src->prg, src->prg_len);
    dst->prg_len = src->prg_len;
    dst->prg_ram = dup_bytes(src->prg_ram, src->prg_ram_len);
    dst->prg_ram_len = src->prg_ram_len;
    dst->chr = dup_bytes(src->chr, src->chr_len);
    dst->chr_len = src->chr_len;
}

static void mapper_free(Mapper *m) {
    free(m->prg);
    free(m->prg_ram);
    free(m->chr);
    memset(m, 0, sizeof *m);
}

static uint8_t mapper_read(Mapper *m, uint16_t addr) {
    assert(m->prg_ram_len == 8 * 1024);
    assert(m->prg_len == 16 * 1024 || m->prg_len == 32 * 1024);

    if (addr >= 0x6000 && addr <= 0x7FFF)
        return m->prg_ram[chk(addr - 0x6000, m->prg_ram_len)];
    if (addr >= 0x8000 && addr <= 0xBFFF)
        return m->prg[chk(addr - 0x8000, m->prg_len)];
    if (addr >= 0xC000) {
        if (m->prg_len == 32 * 1024)
            return m->prg[chk(addr - 0x8000, m->prg_len)];
        return m->prg[chk(mirror_addr(0x8000, 0xBFFF, 0xC000, addr) - 0x8000, m->prg_len)];
    }
    fail("Reference to invalid mapper 0 address");
}

static void mapper_write(Mapper *m, uint16_t addr, uint8_t val) {
    assert(m->prg_ram_len == 8 * 1024);
    assert(m->prg_len == 16 * 1024 || m->prg_len == 32 * 1024);

    if (addr >= 0x6000 && addr <= 0x7FFF) {
        m->prg_ram[chk(addr - 0x6000, m->prg_ram_len)] = val;
        return;
    }
    if (addr >= 0x8000 && addr <= 0xBFFF) {
        m->prg[chk(addr - 0x8000, m->prg_len)] = val;
        return;
    }
    if (addr >= 0xC000) {
        if (m->prg_len == 32 * 1024)
            m->prg[chk(addr - 0x8000, m->prg_len)] = val;
        else
            m->prg[chk(mirror_addr(0x8000, 0xBFFF, 0xC000, addr) - 0x8000, m->prg_len)] = val;
        return;
    }
    fail("Reference to invalid mapper 0 address");
}

static uint8_t mapper_read_ppu(Mapper *m, uint16_t addr) {
    if (addr <= 0x1FFF)
        return m->chr[chk(addr, m->chr_len)];
    fail("Reference to invalid mapper 0 ppu address");
}

static void mapper_write_ppu(Mapper *m, uint16_t addr, uint8_t val) {
    if (addr <= 0x1FFF) {
        m->chr[chk(addr, m->chr_len)] = val;
        return;
    }
    fail("Reference to invalid mapper 0 ppu address");
}

static bool mapper_horizontal_mirroring(const Mapper *m, bool rom_val) {
    (void)m;
    return rom_val;
}

/* ---------------------------------------------------------------- Controller */

typedef struct {
    bool up, down, left, right, a, b, start, select;
    bool strobe;
    uint8_t count;
} Controller;

static uint8_t controller_read(Controller *c) {
    bool res;
    if (c->strobe) {
        res = c->a;
    } else {
        if (c->count <= 8)
            c->count += 1;
        switch (c->count) {
        case 1: res = c->a; break;
        case 2: res = c->b; break;
        case 3: res = c->select; break;
        case 4: res = c->start; break;
        case 5: res = c->up; break;
        case 6: res = c->down; break;
        case 7: res = c->left; break;
        case 8: res = c->right; break;
        default: res = true; break;
        }
    }
    return res ? 1 : 0;
}

static void controller_write(Controller *c, uint8_t val) {
    c->strobe = (val & 0x01) > 0;
    c->count = 0;
}

/* ----------------------------------------------------------------------- PPU */

typedef struct {
    uint32_t count;
    uint8_t nametable;
    uint8_t ppuscroll_x;
    uint8_t ppuscroll_y;
    uint8_t spritetable;
    uint8_t backgroundtable;
    uint8_t sprite_size;
    bool greyscale;
    bool show_background;
    bool show_sprites;
    Mapper mapper;
} MidState;

typedef struct {
    uint8_t vram[2 * 1024];
    uint8_t palette_rame[32];
    bool horiz_mapping;

    uint8_t oamaddr;
    uint8_t oam[256];

    uint8_t ppuscroll_x;
    uint8_t ppuscroll_y;
    bool ppuscroll_ppuaddr_pick;

    uint8_t ppuaddr_hi;
    uint8_t ppuaddr_lo;

    uint8_t nametable;
    uint8_t vram_inc;
    uint8_t spritetable;
    uint8_t backgroundtable;
    uint8_t sprite_size;
    bool ppu_mss;
    bool generate_nmi;

    uint8_t ppu_chr_rom_delay_buffer;
    bool enable_ppu_chr_delay;

    bool greyscale;
    bool mask_left_background;
    bool mask_left_sprites;
    bool show_background;
    bool show_sprites;
    bool em_red;
    bool em_green;
    bool em_blue;

    bool sprite_overflow;
    bool sprite_0_hit;
    bool vertical_blanking;

    uint8_t output_canvas[PPU_HEIGHT][PPU_WIDTH][4];
    uint16_t sprite_output[PPU_WIDTH][PPU_HEIGHT];
    uint16_t bg_output[PPU_WIDTH][PPU_HEIGHT];
    bool sprite_priority[PPU_WIDTH][PPU_HEIGHT];
    bool pixel_greyscale[PPU_WIDTH][PPU_HEIGHT];
    bool has_blanked;

    MidState *states;
    size_t nstates;
    size_t cap_states;
    bool has_drawn_sprite0_background;

    uint32_t last_ticked_scanline;
} Ppu;

/* -------------------------------------------------------------------- CPU */

typedef struct {
    uint8_t a, x, y, s;
    uint16_t pc;
    bool negative, overflow, interrupt, irq_disable, zero, carry, decimal;
    uint32_t count;
    bool nmi_waiting;
    bool irq_waiting;
} Cpu;

/* ------------------------------------------------------------- Chipset */

typedef struct {
    uint16_t addr;
    uint8_t val;
} PpuWrite;

typedef struct {
    Mapper mapper;
    uint8_t ram[2 * 1024];
    Ppu ppu;
    Controller controller1;
    Controller controller2;

    bool ppu_dma_requested;
    uint8_t ppu_dma_val;

    PpuWrite ppu_writes_requested[64];
    size_t nppu_writes;
} Chipset;

struct Emu {
    Cpu cpu;
    Chipset chip;
    uint8_t canvas[PPU_HEIGHT][PPU_WIDTH][4];
    uint8_t video[EMU_FRAME_BYTES];
};

/* ------------------------------------------------------- MidState / Ppu */

static void ppu_push_state(Ppu *p, const Cpu *cpu, Mapper *m) {
    if (p->nstates == p->cap_states) {
        size_t cap = p->cap_states ? p->cap_states * 2 : 16;
        MidState *ns = realloc(p->states, cap * sizeof *ns);
        if (!ns)
            fail("out of memory");
        p->states = ns;
        p->cap_states = cap;
    }
    MidState *s = &p->states[p->nstates++];
    s->count = cpu->count;
    s->nametable = p->nametable;
    s->ppuscroll_x = p->ppuscroll_x;
    s->ppuscroll_y = p->ppuscroll_y;
    s->spritetable = p->spritetable;
    s->backgroundtable = p->backgroundtable;
    s->sprite_size = p->sprite_size;
    s->greyscale = p->greyscale;
    s->show_background = p->show_background;
    s->show_sprites = p->show_sprites;
    mapper_copy(&s->mapper, m);
}

static void ppu_states_clear(Ppu *p) {
    for (size_t i = 0; i < p->nstates; i++)
        mapper_free(&p->states[i].mapper);
    p->nstates = 0;
}

static void ppu_increment_ppuaddr(Ppu *p) {
    uint16_t addr = (uint16_t)((p->ppuaddr_lo & 0x00FF) + ((p->ppuaddr_hi & 0xFF) << 8));
    addr = (uint16_t)(addr + (p->vram_inc == 0 ? 1 : 32));
    p->ppuaddr_lo = (uint8_t)(addr & 0x00FF);
    p->ppuaddr_hi = (uint8_t)((addr & 0xFF00) >> 8);
}

/* Mem for Ppu: read */
static uint8_t ppu_read(Ppu *p, Mapper *m, uint16_t addr) {
    if (addr <= 0x1FFF) {
        if (p->enable_ppu_chr_delay) {
            uint8_t val = p->ppu_chr_rom_delay_buffer;
            p->ppu_chr_rom_delay_buffer = mapper_read_ppu(m, addr);
            return val;
        }
        return mapper_read_ppu(m, addr);
    }
    if (addr <= 0x23FF)
        return p->vram[chk(addr - 0x2000, sizeof p->vram)];
    if (addr <= 0x27FF) {
        if (mapper_horizontal_mirroring(m, p->horiz_mapping))
            return p->vram[chk(addr - 0x2400, sizeof p->vram)];
        return p->vram[chk(addr - 0x2000, sizeof p->vram)];
    }
    if (addr <= 0x2BFF) {
        if (mapper_horizontal_mirroring(m, p->horiz_mapping))
            return p->vram[chk(addr - 0x2400, sizeof p->vram)];
        return p->vram[chk(addr - 0x2800, sizeof p->vram)];
    }
    if (addr <= 0x2FFF)
        return p->vram[chk(addr - 0x2800, sizeof p->vram)];
    if (addr <= 0x3EFF)
        return ppu_read(p, m, mirror_addr(0x2000, 0x2FFF, 0x3000, addr));
    if (addr == 0x3F10)
        return ppu_read(p, m, 0x3F00);
    if (addr == 0x3F14)
        return ppu_read(p, m, 0x3F04);
    if (addr == 0x3F18)
        return ppu_read(p, m, 0x3F08);
    if (addr == 0x3F1C)
        return ppu_read(p, m, 0x3F0C);
    if (addr <= 0x3F1F)
        return p->palette_rame[chk(addr - 0x3F00, sizeof p->palette_rame)];
    if (addr <= 0x3FFF)
        return ppu_read(p, m, mirror_addr(0x3F20, 0x3FFF, 0x3F00, addr));
    return ppu_read(p, m, mirror_addr(0x0000, 0x3FFF, 0x4000, addr));
}

/* Mem for Ppu: write */
static void ppu_write(Ppu *p, Mapper *m, uint16_t addr, uint8_t val) {
    if (addr <= 0x1FFF) {
        mapper_write_ppu(m, addr, val);
        return;
    }
    if (addr <= 0x23FF) {
        p->vram[chk(addr - 0x2000, sizeof p->vram)] = val;
        return;
    }
    if (addr <= 0x27FF) {
        if (mapper_horizontal_mirroring(m, p->horiz_mapping))
            p->vram[chk(addr - 0x2400, sizeof p->vram)] = val;
        else
            p->vram[chk(addr - 0x2000, sizeof p->vram)] = val;
        return;
    }
    if (addr <= 0x2BFF) {
        if (mapper_horizontal_mirroring(m, p->horiz_mapping))
            p->vram[chk(addr - 0x2400, sizeof p->vram)] = val;
        else
            p->vram[chk(addr - 0x2800, sizeof p->vram)] = val;
        return;
    }
    if (addr <= 0x2FFF) {
        p->vram[chk(addr - 0x2800, sizeof p->vram)] = val;
        return;
    }
    if (addr <= 0x3EFF) {
        ppu_write(p, m, mirror_addr(0x2000, 0x2FFF, 0x3000, addr), val);
        return;
    }
    if (addr == 0x3F10) {
        ppu_write(p, m, 0x3F00, val);
        return;
    }
    if (addr == 0x3F14) {
        ppu_write(p, m, 0x3F04, val);
        return;
    }
    if (addr == 0x3F18) {
        ppu_write(p, m, 0x3F08, val);
        return;
    }
    if (addr == 0x3F1C) {
        ppu_write(p, m, 0x3F0C, val);
        return;
    }
    if (addr <= 0x3F1F) {
        p->palette_rame[chk(addr - 0x3F00, sizeof p->palette_rame)] = val;
        return;
    }
    if (addr <= 0x3FFF) {
        ppu_write(p, m, mirror_addr(0x3F20, 0x3FFF, 0x3F00, addr), val);
        return;
    }
    ppu_write(p, m, mirror_addr(0x0000, 0x3FFF, 0x4000, addr), val);
}

static uint8_t ppu_read_main(Ppu *p, Mapper *m, uint16_t addr) {
    switch (addr) {
    case 0x2002: {
        bool blanking = p->vertical_blanking;
        p->vertical_blanking = false;
        p->ppuscroll_ppuaddr_pick = false;
        return (uint8_t)(((uint8_t)blanking << 7) + ((uint8_t)p->sprite_0_hit << 6) +
                         ((uint8_t)p->sprite_overflow << 5));
    }
    case 0x2003:
        return p->oam[p->oamaddr];
    case 0x2007: {
        uint16_t a = (uint16_t)((p->ppuaddr_lo & 0x00FF) + ((p->ppuaddr_hi & 0xFF) << 8));
        p->enable_ppu_chr_delay = true;
        uint8_t val = ppu_read(p, m, a);
        p->enable_ppu_chr_delay = false;
        ppu_increment_ppuaddr(p);
        return val;
    }
    default:
        fail("Read from invalid main address");
    }
}

static void ppu_write_main(Ppu *p, Mapper *m, uint16_t addr, uint8_t val, const Cpu *cpu) {
    switch (addr) {
    case 0x2000:
        p->nametable = val & 0x03;
        p->vram_inc = (val & 0x04) >> 2;
        p->spritetable = (val & 0x08) >> 3;
        p->backgroundtable = (val & 0x10) >> 4;
        p->sprite_size = (val & 0x20) >> 5;
        p->ppu_mss = (val & 0x40) > 0;
        p->generate_nmi = (val & 0x80) > 0;
        ppu_push_state(p, cpu, m);
        break;
    case 0x2001:
        p->greyscale = (val & 0x01) > 0;
        p->mask_left_background = (val & 0x02) > 0;
        p->mask_left_sprites = (val & 0x04) > 0;
        p->show_background = (val & 0x08) > 0;
        p->show_sprites = (val & 0x10) > 0;
        p->em_red = (val & 0x20) > 0;
        p->em_green = (val & 0x40) > 0;
        p->em_blue = (val & 0x80) > 0;
        ppu_push_state(p, cpu, m);
        break;
    case 0x2003:
        p->oamaddr = val;
        break;
    case 0x2004:
        p->oam[p->oamaddr] = val;
        p->oamaddr = (uint8_t)(p->oamaddr + 1);
        break;
    case 0x2005:
        if (p->ppuscroll_ppuaddr_pick) {
            p->ppuscroll_y = val;
        } else {
            p->ppuscroll_x = val;
            ppu_push_state(p, cpu, m);
        }
        p->ppuscroll_ppuaddr_pick = !p->ppuscroll_ppuaddr_pick;
        break;
    case 0x2006:
        if (p->ppuscroll_ppuaddr_pick) {
            p->ppuaddr_lo = val;
            /* Hack so that full scanline emulation is not needed. */
            uint8_t nametable = (p->ppuaddr_hi & 0x0C) >> 2;
            if (nametable != p->nametable) {
                p->nametable = nametable;
                ppu_push_state(p, cpu, m);
            }
        } else {
            p->ppuaddr_hi = val;
        }
        p->ppuscroll_ppuaddr_pick = !p->ppuscroll_ppuaddr_pick;
        break;
    case 0x2007: {
        uint16_t a = (uint16_t)((p->ppuaddr_lo & 0x00FF) + ((p->ppuaddr_hi & 0xFF) << 8));
        ppu_write(p, m, a, val);
        ppu_increment_ppuaddr(p);
        break;
    }
    default:
        fail("Write to invalid main address");
    }
}

typedef struct {
    uint8_t x;
    uint16_t y;
    uint8_t height;
    uint16_t pattern_addr;
    uint16_t palette;
    bool priority;
    bool fh;
    bool fv;
} SpriteAttrs;

static SpriteAttrs get_sprite_attrs(const Ppu *p, uint8_t s, size_t state_idx) {
    const MidState *state = &p->states[chk(state_idx, p->nstates)];
    SpriteAttrs r;
    uint8_t height;
    uint8_t table;
    uint8_t idx;

    r.y = (uint16_t)(p->oam[(uint8_t)(p->oamaddr + 4 * s)] + 1);

    if (state->sprite_size == 0) {
        height = 8;
        table = state->spritetable;
        idx = p->oam[(uint8_t)(p->oamaddr + 4 * s + 1)];
    } else if (state->sprite_size == 1) {
        uint8_t val = p->oam[(uint8_t)(p->oamaddr + 4 * s + 1)];
        height = 16;
        table = val & 0x01;
        idx = val & 0xFE;
    } else {
        fail("sprite size");
    }

    uint16_t pattern_base;
    switch (table) {
    case 0: pattern_base = 0x0000; break;
    case 1: pattern_base = 0x1000; break;
    default: fail("sprite pattern table");
    }
    r.pattern_addr = (uint16_t)(pattern_base + 16 * idx);

    uint8_t flags = p->oam[(uint8_t)(p->oamaddr + 4 * s + 2)];
    r.palette = (uint16_t)(0x3F10 + (((uint16_t)(flags & 0x03)) << 2));
    r.priority = (flags & 0x20) == 0;
    r.fh = (flags & 0x40) > 0;
    r.fv = (flags & 0x80) > 0;

    r.x = p->oam[(uint8_t)(p->oamaddr + 4 * s + 3)];
    r.height = height;
    return r;
}

static void ppu_draw_tile(Ppu *p, Mapper *m, size_t state_idx, int nametable_sel, int tile_x, int tile_y,
                          int sx0, int sy0, int sx1, int sy1, int x_offset, int y_offset) {
    uint16_t nametable;
    switch (nametable_sel) {
    case 0: nametable = 0x2000; break;
    case 1: nametable = 0x2400; break;
    case 2: nametable = 0x2800; break;
    case 3: nametable = 0x2C00; break;
    default: fail("Name table not recognized");
    }

    uint16_t bg_pattern;
    switch (p->states[chk(state_idx, p->nstates)].backgroundtable) {
    case 0: bg_pattern = 0x0000; break;
    case 1: bg_pattern = 0x1000; break;
    default: fail("Background table not recognized");
    }

    uint8_t pattern_number = ppu_read(p, m, (uint16_t)(nametable + tile_x + 32 * tile_y));

    int attr_x = tile_x / 4;
    int attr_y = tile_y / 4;
    int over_x = (tile_x / 2) % 2;
    int over_y = (tile_y / 2) % 2;
    uint8_t attr = ppu_read(p, m, (uint16_t)(nametable + 0x3C0 + attr_x + 8 * attr_y));
    uint8_t mask = (uint8_t)(0x03 << (4 * over_y + 2 * over_x));
    uint8_t colour_bits = (uint8_t)(((attr & mask) >> (4 * over_y + 2 * over_x)) << 2);

    uint16_t pattern_addr = (uint16_t)(bg_pattern + 16 * pattern_number);
    for (int y = y_offset; y <= sy1 - sy0 + y_offset; y++) {
        uint8_t lo = ppu_read(p, m, (uint16_t)(pattern_addr + y));
        uint8_t hi = ppu_read(p, m, (uint16_t)(pattern_addr + y + 8));

        for (int x = x_offset; x <= sx1 - sx0 + x_offset; x++) {
            uint8_t mask_px = (uint8_t)(0x01 << (7 - x));
            uint16_t palette_idx = (uint16_t)(((lo & mask_px) >> (7 - x)) + (((hi & mask_px) >> (7 - x)) << 1));
            if (palette_idx != 0)
                palette_idx = (uint16_t)(palette_idx + colour_bits);

            size_t ox = chk((size_t)(x + sx0 - x_offset), PPU_WIDTH);
            size_t oy = chk((size_t)(y + sy0 - y_offset), PPU_HEIGHT);
            if (p->states[state_idx].show_background)
                p->bg_output[ox][oy] = (uint16_t)(0x3F00 + palette_idx);

            p->pixel_greyscale[ox][oy] = p->states[state_idx].greyscale;
        }
    }
}

static void ppu_draw_with_state(Ppu *p, size_t state_idx, uint16_t state_start_y, uint16_t state_end_y) {
    Mapper current_mapper;
    mapper_copy(&current_mapper, &p->states[chk(state_idx, p->nstates)].mapper);

    int sx = p->states[state_idx].ppuscroll_x;
    int sy = p->states[state_idx].ppuscroll_y;
    int base_nt = p->states[state_idx].nametable;
    int base_nt_x;
    int base_nt_y;
    switch (base_nt) {
    case 0: base_nt_x = 0; base_nt_y = 0; break;
    case 1: base_nt_x = 1; base_nt_y = 0; break;
    case 2: base_nt_x = 0; base_nt_y = 1; break;
    case 3: base_nt_x = 1; base_nt_y = 1; break;
    default: fail("base nametable");
    }

    int sy_start = state_start_y;
    int sy_end = state_end_y;

    for (int screen_x = 0; screen_x < 33; screen_x++) {
        for (int screen_y = sy_start / 8; screen_y <= sy_end / 8 + 1; screen_y++) {
            int x_nt = ((sx / 8 + screen_x + 32 * base_nt_x) % 64) / 32;
            int y_nt = ((sy / 8 + screen_y + 30 * base_nt_y) % 60) / 30;

            int n;
            if (x_nt == 0 && y_nt == 0)
                n = 0;
            else if (x_nt == 1 && y_nt == 0)
                n = 1;
            else if (x_nt == 0 && y_nt == 1)
                n = 2;
            else if (x_nt == 1 && y_nt == 1)
                n = 3;
            else
                fail("nametable selection");

            int tile_x = ((sx / 8 + screen_x + 32 * base_nt_x) % 64) % 32;
            int tile_y = ((sy / 8 + screen_y + 30 * base_nt_y) % 60) % 30;

            int start_x;
            int off_x;
            if (screen_x == 0) {
                start_x = 0;
                off_x = sx % 8;
            } else {
                start_x = screen_x * 8 - sx % 8;
                off_x = 0;
            }

            int start_y;
            int off_y;
            if (screen_y * 8 + 8 - sy % 8 <= sy_start) {
                continue;
            } else if (screen_y * 8 <= sy_start + sy % 8) {
                start_y = sy_start;
                off_y = sy_start + sy % 8 - screen_y * 8;
            } else {
                start_y = screen_y * 8 - sy % 8;
                off_y = 0;
            }

            int end_x = screen_x * 8 + 8 - sx % 8 - 1;
            if (end_x > PPU_WIDTH - 1)
                end_x = PPU_WIDTH - 1;
            int end_y = screen_y * 8 + 8 - sy % 8 - 1;
            if (end_y > sy_end)
                end_y = sy_end;

            if (start_x > end_x || start_y > end_y)
                continue;

            ppu_draw_tile(p, &current_mapper, state_idx, n, tile_x, tile_y, start_x, start_y, end_x, end_y,
                          off_x, off_y);
        }
    }

    for (int s = 0; s < 64; s++) {
        SpriteAttrs a = get_sprite_attrs(p, (uint8_t)s, state_idx);
        if (a.y >= 0xF0)
            continue;
        if (a.y <= 2)
            continue;
        if (a.y > state_end_y)
            continue;
        if ((int)a.y + (int)a.height < (int)state_start_y)
            continue;

        for (int i = 0; i < a.height / 8; i++) {
            for (int py = 0; py < 8; py++) {
                uint8_t lo = ppu_read(p, &current_mapper, (uint16_t)(a.pattern_addr + 16 * i + py));
                uint8_t hi = ppu_read(p, &current_mapper, (uint16_t)(a.pattern_addr + 16 * i + py + 8));

                for (int px = 0; px < 8; px++) {
                    int real_x = !a.fh ? a.x + px : a.x + 7 - px;
                    int real_y = !a.fv ? a.y + (8 * i + py)
                                       : a.y + (8 * (a.height / 8) - 1) - (8 * i + py);

                    if (real_x >= PPU_WIDTH || real_y > sy_end || real_y < sy_start)
                        continue;

                    size_t rx = chk((size_t)real_x, PPU_WIDTH);
                    size_t ry = chk((size_t)real_y, PPU_HEIGHT);
                    if (p->sprite_output[rx][ry] != 0)
                        continue;

                    uint8_t mask = (uint8_t)(0x01 << (7 - px));
                    uint8_t palette_idx = (uint8_t)(((lo & mask) >> (7 - px)) + (((hi & mask) >> (7 - px)) << 1));
                    if (palette_idx == 0)
                        continue;

                    if (p->states[state_idx].show_sprites) {
                        p->sprite_output[rx][ry] = (uint16_t)(palette_idx + a.palette);
                        p->sprite_priority[rx][ry] = a.priority;
                    }
                }
            }
        }
    }

    mapper_free(&current_mapper);
}

static void ppu_prepare_draw(Ppu *p, Mapper *m) {
    p->last_ticked_scanline = 0;

    for (int x = 0; x < PPU_WIDTH; x++)
        for (int y = 0; y < PPU_HEIGHT; y++) {
            p->sprite_output[x][y] = 0;
            p->bg_output[x][y] = 0;
            p->sprite_priority[x][y] = false;
        }

    for (size_t i = 0; i < p->nstates; i++) {
        uint16_t start_y = (uint16_t)((uint16_t)(p->states[i].count * 3 / 341) - VBL);
        uint16_t end_y;
        if (i < p->nstates - 1) {
            uint16_t next = (uint16_t)((uint16_t)(p->states[i + 1].count * 3 / 341) - VBL);
            uint16_t v = next > 1 ? next : 1;
            end_y = v < PPU_HEIGHT ? v : PPU_HEIGHT;
        } else {
            end_y = PPU_HEIGHT;
        }
        if (end_y < start_y)
            continue;

        ppu_draw_with_state(p, i, start_y, (uint16_t)(end_y - 1));
    }

    for (int x = 0; x < PPU_WIDTH; x++)
        for (int y = 0; y < PPU_HEIGHT; y++) {
            uint16_t sprite = p->sprite_output[x][y];
            uint16_t bg = p->bg_output[x][y];

            uint8_t mask = p->pixel_greyscale[x][y] ? 0x30 : 0xFF;

            uint16_t p_idx = (sprite > 0 && (p->sprite_priority[x][y] || bg == 0x3F00)) ? sprite : bg;

            size_t hsv = (size_t)(ppu_read(p, m, p_idx) & mask);
            uint8_t *px = p->output_canvas[y][x];
            px[0] = hsv * 3 < sizeof PALETTE ? PALETTE[hsv * 3] : 0;
            px[1] = hsv * 3 + 1 < sizeof PALETTE ? PALETTE[hsv * 3 + 1] : 0;
            px[2] = hsv * 3 + 2 < sizeof PALETTE ? PALETTE[hsv * 3 + 2] : 0;
            px[3] = 0xFF;
        }
}

static void ppu_tick(Ppu *p, Cpu *cpu, Mapper *m) {
    uint32_t y = cpu->count * 3 / 341;

    while (p->last_ticked_scanline < y && p->last_ticked_scanline < 262 && (cpu->count * 3) % 341 > 260)
        p->last_ticked_scanline += 1;

    if (y < VBL && !p->has_blanked) {
        p->has_blanked = true;
        p->vertical_blanking = true;
        p->oamaddr = 0;

        if (p->generate_nmi)
            cpu->nmi_waiting = true;
    }

    if (y >= VBL && p->has_blanked) {
        p->has_blanked = false;
        p->vertical_blanking = false;
        p->has_drawn_sprite0_background = false;
        p->sprite_0_hit = false;

        ppu_states_clear(p);
        ppu_push_state(p, cpu, m);
    }

    uint32_t sprite_0_y = (uint32_t)p->oam[p->oamaddr] + 1u;
    if (p->show_sprites && p->show_background && !p->sprite_0_hit && y >= sprite_0_y + VBL + 1 &&
        y < sprite_0_y + VBL + 1 + 8) {
        if (p->nstates == 0)
            fail("states.len()-1 underflow");
        size_t idx = p->nstates - 1;
        SpriteAttrs s0 = get_sprite_attrs(p, 0, idx);

        if (!p->has_drawn_sprite0_background) {
            p->has_drawn_sprite0_background = true;

            p->show_sprites = false;
            ppu_draw_with_state(p, idx, (uint16_t)sprite_0_y, (uint16_t)(sprite_0_y + 8));
            p->show_sprites = true;
        }

        uint16_t py = (uint16_t)(y - sprite_0_y - VBL - 1);

        /* Who cares about 16px sprites! */
        uint8_t lo = ppu_read(p, m, (uint16_t)(s0.pattern_addr + py));
        uint8_t hi = ppu_read(p, m, (uint16_t)(s0.pattern_addr + py + 8));

        for (int px = 0; px < 8; px++) {
            uint8_t mask = (uint8_t)(0x01 << (7 - px));
            bool solid = (((lo & mask) >> (7 - px)) + (((hi & mask) >> (7 - px)) << 1)) != 0;

            if (!solid)
                continue;
            if ((p->bg_output[chk(s0.x + px, PPU_WIDTH)][chk(sprite_0_y + py, PPU_HEIGHT)] & 0x03) == 0)
                continue;

            p->sprite_0_hit = true;
            break;
        }
    }
}

/* ----------------------------------------------------------- Chipset memory */

static uint8_t memory_read(Chipset *c, uint16_t addr) {
    if (addr <= 0x07FF)
        return c->ram[addr];
    if (addr <= 0x1FFF)
        return memory_read(c, mirror_addr(0x0000, 0x07FF, 0x0800, addr));
    if (addr >= 0x4020)
        return mapper_read(&c->mapper, addr);
    fail("Reference to invalid main address");
}

static void memory_write(Chipset *c, uint16_t addr, uint8_t val) {
    if (addr <= 0x07FF) {
        c->ram[addr] = val;
        return;
    }
    if (addr <= 0x1FFF) {
        memory_write(c, mirror_addr(0x0000, 0x07FF, 0x0800, addr), val);
        return;
    }
    if (addr >= 0x4020) {
        mapper_write(&c->mapper, addr, val);
        return;
    }
    fail("Reference to invalid main address");
}

static uint8_t chip_read(Chipset *c, uint16_t addr) {
    if (addr >= 0x2000 && addr <= 0x2007)
        return ppu_read_main(&c->ppu, &c->mapper, addr);
    if (addr >= 0x2008 && addr <= 0x3FFF)
        return chip_read(c, mirror_addr(0x2000, 0x2007, 0x2008, addr));
    if (addr == 0x4014)
        return ppu_read_main(&c->ppu, &c->mapper, addr);
    if (addr == 0x4016)
        return controller_read(&c->controller1);
    if (addr >= 0x4000 && addr <= 0x4017)
        return 0;
    return memory_read(c, addr);
}

static uint16_t chip_read16(Chipset *c, uint16_t addr) {
    uint8_t lo = chip_read(c, addr);
    uint8_t hi = chip_read(c, (uint16_t)(addr + 1));
    return (uint16_t)(lo + ((uint16_t)hi << 8));
}

static uint16_t memory_read16(Chipset *c, uint16_t addr) {
    uint8_t lo = memory_read(c, addr);
    uint8_t hi = memory_read(c, (uint16_t)(addr + 1));
    return (uint16_t)(lo + ((uint16_t)hi << 8));
}

static void chip_write(Chipset *c, uint16_t addr, uint8_t val) {
    if (addr >= 0x2000 && addr <= 0x2007) {
        if (c->nppu_writes >= sizeof c->ppu_writes_requested / sizeof c->ppu_writes_requested[0])
            fail("ppu write queue overflow");
        c->ppu_writes_requested[c->nppu_writes].addr = addr;
        c->ppu_writes_requested[c->nppu_writes].val = val;
        c->nppu_writes++;
        return;
    }
    if (addr >= 0x2008 && addr <= 0x3FFF) {
        chip_write(c, mirror_addr(0x2000, 0x2007, 0x2008, addr), val);
        return;
    }
    if (addr == 0x4014) {
        c->ppu_dma_requested = true;
        c->ppu_dma_val = val;
        return;
    }
    if (addr == 0x4016) {
        controller_write(&c->controller1, val);
        controller_write(&c->controller2, val);
        return;
    }
    if (addr >= 0x4000 && addr <= 0x4017)
        return;
    memory_write(c, addr, val);
}

/* Ppu::ppudma: copies a page of CPU memory into OAM. */
static void ppudma(Ppu *p, Chipset *c, uint8_t val, Cpu *cpu) {
    cpu->count += 1;
    cpu->count += cpu->count % 2;
    cpu->count += 512;

    uint16_t addr = (uint16_t)((val & 0x00FF) << 8);
    for (int i = 0; i <= 255; i++) {
        uint8_t v = memory_read(c, (uint16_t)(addr + i));
        p->oam[(uint8_t)(p->oamaddr + i)] = v;
    }
}

/* -------------------------------------------------------------------- CPU */

typedef enum { AR_VAL, AR_ADDR, AR_ACC, AR_X, AR_Y } ArKind;

typedef struct {
    ArKind kind;
    uint8_t val;
    uint16_t addr;
} AddrRes;

static AddrRes ar_val(uint8_t v) {
    AddrRes r = { AR_VAL, v, 0 };
    return r;
}

static AddrRes ar_addr(uint16_t a) {
    AddrRes r = { AR_ADDR, 0, a };
    return r;
}

static AddrRes ar_kind(ArKind k) {
    AddrRes r = { k, 0, 0 };
    return r;
}

static uint8_t ar_read(AddrRes r, const Cpu *cpu, Chipset *mem) {
    switch (r.kind) {
    case AR_VAL: return r.val;
    case AR_ADDR: return chip_read(mem, r.addr);
    case AR_ACC: return cpu->a;
    case AR_X: return cpu->x;
    case AR_Y: return cpu->y;
    }
    fail("unreachable address mode result");
}

static void ar_write(AddrRes r, Cpu *cpu, Chipset *mem, uint8_t val) {
    switch (r.kind) {
    case AR_ADDR: chip_write(mem, r.addr, val); return;
    case AR_ACC: cpu->a = val; return;
    case AR_X: cpu->x = val; return;
    case AR_Y: cpu->y = val; return;
    default: fail("Attempt to write to a read-only AddressModeResult");
    }
}

typedef AddrRes (*AddrMode)(Cpu *, Chipset *, bool);
typedef void (*AluOp)(Cpu *, Chipset *, AddrMode);

static void set_zn(Cpu *cpu, uint8_t v) {
    cpu->zero = v == 0;
    cpu->negative = (v & 0x80) > 0;
}

static AddrRes mode_immediate(Cpu *cpu, Chipset *mem, bool page_matters) {
    (void)page_matters;
    cpu->pc = (uint16_t)(cpu->pc + 1);
    cpu->count += 2;
    return ar_val(chip_read(mem, (uint16_t)(cpu->pc - 1)));
}

static AddrRes mode_zero_page(Cpu *cpu, Chipset *mem, bool page_matters) {
    (void)page_matters;
    uint8_t arg = chip_read(mem, cpu->pc);
    cpu->pc = (uint16_t)(cpu->pc + 1);
    cpu->count += 3;
    return ar_addr(arg);
}

static AddrRes mode_zero_page_x(Cpu *cpu, Chipset *mem, bool page_matters) {
    (void)page_matters;
    uint8_t arg = chip_read(mem, cpu->pc);
    cpu->pc = (uint16_t)(cpu->pc + 1);
    cpu->count += 4;
    return ar_addr((uint16_t)((arg + cpu->x) % 256));
}

static AddrRes mode_zero_page_y(Cpu *cpu, Chipset *mem, bool page_matters) {
    (void)page_matters;
    uint8_t arg = chip_read(mem, cpu->pc);
    cpu->pc = (uint16_t)(cpu->pc + 1);
    cpu->count += 4;
    return ar_addr((uint16_t)((arg + cpu->y) % 256));
}

static AddrRes mode_absolute(Cpu *cpu, Chipset *mem, bool page_matters) {
    (void)page_matters;
    cpu->pc = (uint16_t)(cpu->pc + 2);
    cpu->count += 4;
    return ar_addr(chip_read16(mem, (uint16_t)(cpu->pc - 2)));
}

static AddrRes mode_absolute_x(Cpu *cpu, Chipset *mem, bool page_matters) {
    uint16_t arg = chip_read16(mem, cpu->pc);
    cpu->pc = (uint16_t)(cpu->pc + 2);
    cpu->count += 4;

    uint16_t addr = (uint16_t)(arg + cpu->x);
    if (!page_matters || addr / 256 != arg / 256)
        cpu->count += 1;
    return ar_addr(addr);
}

static AddrRes mode_absolute_y(Cpu *cpu, Chipset *mem, bool page_matters) {
    uint16_t arg = chip_read16(mem, cpu->pc);
    cpu->pc = (uint16_t)(cpu->pc + 2);
    cpu->count += 4;

    uint16_t addr = (uint16_t)(arg + cpu->y);
    if (!page_matters || addr / 256 != arg / 256)
        cpu->count += 1;
    return ar_addr(addr);
}

static AddrRes mode_indirect_x(Cpu *cpu, Chipset *mem, bool page_matters) {
    (void)page_matters;
    uint8_t arg = chip_read(mem, cpu->pc);
    cpu->pc = (uint16_t)(cpu->pc + 1);
    cpu->count += 6;

    int z = arg + cpu->x;
    uint8_t lo = chip_read(mem, (uint16_t)(z % 256));
    uint8_t hi = chip_read(mem, (uint16_t)((z + 1) % 256));
    return ar_addr((uint16_t)(lo + (uint16_t)hi * 256));
}

static AddrRes mode_indirect_y(Cpu *cpu, Chipset *mem, bool page_matters) {
    uint8_t arg = chip_read(mem, cpu->pc);
    cpu->pc = (uint16_t)(cpu->pc + 1);
    cpu->count += 5;

    uint8_t blo = chip_read(mem, (uint16_t)(arg % 256));
    uint8_t bhi = chip_read(mem, (uint16_t)((arg + 1) % 256));
    uint16_t base = (uint16_t)(blo + (uint16_t)bhi * 256);

    uint16_t sum = (uint16_t)(base + cpu->y);
    if (!page_matters || sum / 256 != base / 256)
        cpu->count += 1;
    return ar_addr(sum);
}

static AddrRes mode_implied_a(Cpu *cpu, Chipset *mem, bool page_matters) {
    (void)cpu; (void)mem; (void)page_matters;
    return ar_kind(AR_ACC);
}

static AddrRes mode_implied_x(Cpu *cpu, Chipset *mem, bool page_matters) {
    (void)cpu; (void)mem; (void)page_matters;
    return ar_kind(AR_X);
}

static AddrRes mode_implied_y(Cpu *cpu, Chipset *mem, bool page_matters) {
    (void)cpu; (void)mem; (void)page_matters;
    return ar_kind(AR_Y);
}

static AddrRes mode_relative(Cpu *cpu, Chipset *mem, bool page_matters) {
    uint8_t arg = chip_read(mem, cpu->pc);
    cpu->pc = (uint16_t)(cpu->pc + 1);

    uint16_t rel_addr;
    if (arg <= 127)
        rel_addr = (uint16_t)(cpu->pc + arg);
    else
        rel_addr = (uint16_t)(cpu->pc - (uint8_t)(~arg + 1));

    if (!page_matters || cpu->pc / 256 != rel_addr / 256)
        cpu->count = cpu->count + 1;
    cpu->count = cpu->count + 1;
    return ar_addr(rel_addr);
}

static void add_with_carry(Cpu *cpu, uint8_t val) {
    int carry = cpu->carry ? 1 : 0;
    int res = val + cpu->a + carry;
    int res_signed = (int)(int8_t)val + (int)(int8_t)cpu->a + carry;

    cpu->carry = res > 0xFF;
    cpu->overflow = res_signed > 127 || res_signed < -128;
    cpu->a = (uint8_t)(res & 0xFF);

    set_zn(cpu, cpu->a);
}

static void op_adc(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, true);
    uint8_t val = ar_read(r, cpu, mem);
    add_with_carry(cpu, val);
}

static void op_sbc(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, true);
    uint8_t val = ar_read(r, cpu, mem);
    /* Flip the bits for 2s compliment, but rely on carry to add 1. */
    add_with_carry(cpu, (uint8_t)~val);
}

static void op_and(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, true);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->a = cpu->a & val;
    set_zn(cpu, cpu->a);
}

static void op_ora(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, true);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->a = cpu->a | val;
    set_zn(cpu, cpu->a);
}

static void op_eor(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, true);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->a = cpu->a ^ val;
    set_zn(cpu, cpu->a);
}

static void op_asl(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, false);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->count += 2;

    cpu->carry = (val & 0x80) > 0;
    uint8_t result = (uint8_t)(val << 1);
    ar_write(r, cpu, mem, result);
    set_zn(cpu, result);
}

static void op_lsr(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, false);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->count += 2;

    cpu->carry = (val & 0x01) > 0;
    uint8_t result = (uint8_t)((val >> 1) & 0xFF);
    ar_write(r, cpu, mem, result);
    set_zn(cpu, result);
}

static void op_rol(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, false);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->count += 2;

    uint8_t old_carry = cpu->carry ? 1 : 0;
    cpu->carry = (val & 0x80) > 0;
    uint8_t result = (uint8_t)((val << 1) | old_carry);
    ar_write(r, cpu, mem, result);
    set_zn(cpu, result);
}

static void op_ror(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, false);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->count += 2;

    uint8_t old_carry = cpu->carry ? 1 : 0;
    cpu->carry = (val & 0x01) > 0;
    uint8_t result = (uint8_t)((val >> 1) | (old_carry << 7));
    ar_write(r, cpu, mem, result);
    set_zn(cpu, result);
}

static void op_bit(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, true);
    uint8_t val = ar_read(r, cpu, mem);

    cpu->zero = (val & cpu->a) == 0;
    cpu->negative = (val & 0x80) > 0;
    cpu->overflow = (val & 0x40) > 0;
}

static void op_cmp(Cpu *cpu, Chipset *mem, AddrMode mode) {
    uint8_t a = cpu->a;
    bool overflow = cpu->overflow;
    cpu->carry = true;
    op_sbc(cpu, mem, mode);
    cpu->a = a;
    cpu->overflow = overflow;
}

static void op_cpx(Cpu *cpu, Chipset *mem, AddrMode mode) {
    uint8_t a = cpu->a;
    bool overflow = cpu->overflow;
    cpu->carry = true;
    cpu->a = cpu->x;
    op_sbc(cpu, mem, mode);
    cpu->a = a;
    cpu->overflow = overflow;
}

static void op_cpy(Cpu *cpu, Chipset *mem, AddrMode mode) {
    uint8_t a = cpu->a;
    bool overflow = cpu->overflow;
    cpu->carry = true;
    cpu->a = cpu->y;
    op_sbc(cpu, mem, mode);
    cpu->a = a;
    cpu->overflow = overflow;
}

static void op_inc(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, false);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->count += 2;

    uint8_t result = (uint8_t)(((uint16_t)val + 1) & 0xFF);
    ar_write(r, cpu, mem, result);
    set_zn(cpu, result);
}

static void op_dec(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, false);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->count += 2;

    uint8_t result = (uint8_t)(((uint16_t)val - 1) & 0xFF);
    ar_write(r, cpu, mem, result);
    set_zn(cpu, result);
}

static void jump(Cpu *cpu, Chipset *mem, AddrMode mode, bool cond) {
    cpu->count += 2;
    uint32_t count = cpu->count;
    AddrRes r = mode(cpu, mem, true);
    if (r.kind != AR_ADDR)
        fail("Jump instruction address mode must produce an address result!");
    uint16_t val = r.addr;

    if (!cond) {
        /* We read memory to advance the pc, but do not want to advance the count. */
        cpu->count = count;
        return;
    }

    cpu->pc = val;
}

static void op_bpl(Cpu *cpu, Chipset *mem, AddrMode mode) { jump(cpu, mem, mode, !cpu->negative); }
static void op_bmi(Cpu *cpu, Chipset *mem, AddrMode mode) { jump(cpu, mem, mode, cpu->negative); }
static void op_bvc(Cpu *cpu, Chipset *mem, AddrMode mode) { jump(cpu, mem, mode, !cpu->overflow); }
static void op_bvs(Cpu *cpu, Chipset *mem, AddrMode mode) { jump(cpu, mem, mode, cpu->overflow); }
static void op_bcc(Cpu *cpu, Chipset *mem, AddrMode mode) { jump(cpu, mem, mode, !cpu->carry); }
static void op_bcs(Cpu *cpu, Chipset *mem, AddrMode mode) { jump(cpu, mem, mode, cpu->carry); }
static void op_bne(Cpu *cpu, Chipset *mem, AddrMode mode) { jump(cpu, mem, mode, !cpu->zero); }
static void op_beq(Cpu *cpu, Chipset *mem, AddrMode mode) { jump(cpu, mem, mode, cpu->zero); }

static void op_lda(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, true);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->a = val;
    set_zn(cpu, cpu->a);
}

static void op_ldx(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, true);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->x = val;
    set_zn(cpu, cpu->x);
}

static void op_ldy(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes r = mode(cpu, mem, true);
    uint8_t val = ar_read(r, cpu, mem);
    cpu->y = val;
    set_zn(cpu, cpu->y);
}

static void push(Cpu *cpu, Chipset *mem, uint8_t val) {
    chip_write(mem, (uint16_t)(0x0100 + cpu->s), val);
    cpu->s = (uint8_t)(cpu->s - 1);
}

static void push16(Cpu *cpu, Chipset *mem, uint16_t val) {
    push(cpu, mem, (uint8_t)((val & 0xFF00) >> 8));
    push(cpu, mem, (uint8_t)(val & 0x00FF));
}

static uint8_t pull(Cpu *cpu, Chipset *mem) {
    cpu->s = (uint8_t)(cpu->s + 1);
    return chip_read(mem, (uint16_t)(0x0100 + cpu->s));
}

static uint16_t pull16(Cpu *cpu, Chipset *mem) {
    uint8_t lo = pull(cpu, mem);
    uint8_t hi = pull(cpu, mem);
    return (uint16_t)(lo + ((uint16_t)hi << 8));
}

static uint8_t cpu_get_p(const Cpu *cpu) {
    return (uint8_t)(((uint8_t)cpu->negative << 7) + ((uint8_t)cpu->overflow << 6) + (1u << 5) +
                     ((uint8_t)cpu->interrupt << 4) + ((uint8_t)cpu->decimal << 3) +
                     ((uint8_t)cpu->irq_disable << 2) + ((uint8_t)cpu->zero << 1) + ((uint8_t)cpu->carry << 0));
}

static void cpu_set_p(Cpu *cpu, uint8_t val) {
    cpu->negative = (val & 0x80) > 0;
    cpu->overflow = (val & 0x40) > 0;
    cpu->decimal = (val & 0x08) > 0;
    cpu->irq_disable = (val & 0x04) > 0;
    cpu->zero = (val & 0x02) > 0;
    cpu->carry = (val & 0x01) > 0;
}

static void op_sta(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes m = mode(cpu, mem, false);
    uint8_t a = cpu->a;
    ar_write(m, cpu, mem, a);
}

static void op_stx(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes m = mode(cpu, mem, false);
    uint8_t x = cpu->x;
    ar_write(m, cpu, mem, x);
}

static void op_sty(Cpu *cpu, Chipset *mem, AddrMode mode) {
    AddrRes m = mode(cpu, mem, false);
    uint8_t y = cpu->y;
    ar_write(m, cpu, mem, y);
}

static void op_jsr(Cpu *cpu, Chipset *mem, AddrMode mode) {
    cpu->count += 2;
    AddrRes r = mode(cpu, mem, false);
    if (r.kind != AR_ADDR)
        fail("Jsr instruction address mode must produce an address result!");

    uint16_t pc = (uint16_t)(cpu->pc - 1);
    push16(cpu, mem, pc);
    cpu->pc = r.addr;
}

/* cpu.rs manual(): every opcode that is not in the OPCODES table. */
static void manual(Cpu *cpu, Chipset *mem, uint8_t op) {
    cpu->count += 2;
    switch (op) {
    case 0x18: cpu->carry = false; break; /* CLC */
    case 0x38: cpu->carry = true; break; /* SEC */
    case 0x58: cpu->irq_disable = false; break; /* CLI */
    case 0x78: cpu->irq_disable = true; break; /* SEI */
    case 0xB8: cpu->overflow = false; break; /* CLV */
    case 0x1A: case 0x3A: case 0x5A: case 0x7A: case 0xDA: case 0xEA: case 0xFA:
        break; /* NOP */
    case 0xAA: /* TAX */
        cpu->x = cpu->a;
        set_zn(cpu, cpu->x);
        break;
    case 0x8A: /* TXA */
        cpu->a = cpu->x;
        set_zn(cpu, cpu->a);
        break;
    case 0xA8: /* TAY */
        cpu->y = cpu->a;
        set_zn(cpu, cpu->y);
        break;
    case 0x98: /* TYA */
        cpu->a = cpu->y;
        set_zn(cpu, cpu->a);
        break;
    case 0x9A: /* TXS */
        cpu->s = cpu->x;
        break;
    case 0xBA: /* TSX */
        cpu->x = cpu->s;
        set_zn(cpu, cpu->x);
        break;
    case 0x48: { /* PHA */
        cpu->count += 1;
        uint8_t a = cpu->a;
        push(cpu, mem, a);
        break;
    }
    case 0x68: /* PLA */
        cpu->count += 2;
        cpu->a = pull(cpu, mem);
        set_zn(cpu, cpu->a);
        break;
    case 0x08: { /* PHP */
        cpu->count += 1;
        bool interrupt = cpu->interrupt;
        cpu->interrupt = true;
        uint8_t p = cpu_get_p(cpu);
        push(cpu, mem, p);
        cpu->interrupt = interrupt;
        break;
    }
    case 0x28: { /* PLP */
        cpu->count += 2;
        uint8_t p = pull(cpu, mem);
        cpu_set_p(cpu, p);
        break;
    }
    case 0x00: { /* BRK */
        cpu->count += 2;
        uint16_t pc = (uint16_t)(cpu->pc + 1);
        push16(cpu, mem, pc);
        manual(cpu, mem, 0x08); /* PHP */
        cpu->pc = chip_read16(mem, 0xFFFE);
        cpu->irq_disable = true;
        break;
    }
    case 0x60: { /* RTI (as labelled in cpu.rs) */
        manual(cpu, mem, 0x28); /* PLP */
        uint16_t pc = pull16(cpu, mem);
        cpu->pc = pc;
        break;
    }
    case 0x40: { /* RTS (as labelled in cpu.rs) */
        cpu->count += 4;
        uint16_t pc = pull16(cpu, mem);
        cpu->pc = (uint16_t)(pc + 1);
        break;
    }
    case 0x4C: { /* JMP (absolute) */
        cpu->count += 1;
        uint16_t pc = chip_read16(mem, cpu->pc);
        cpu->pc = pc;
        break;
    }
    case 0x6C: { /* JMP (indirect) */
        cpu->count += 3;
        uint16_t addr = chip_read16(mem, cpu->pc);
        uint8_t lo = chip_read(mem, addr);
        uint16_t hi_addr = (uint16_t)((addr & 0xFF00) + ((addr + 1) & 0x00FF));
        uint8_t hi = chip_read(mem, hi_addr);
        cpu->pc = (uint16_t)(lo + ((uint16_t)hi << 8));
        break;
    }
    case 0xF8: cpu->decimal = true; break;
    case 0xD8: cpu->decimal = false; break;
    default:
        fail("Not implemented yet! Op");
    }
}

/* The OPCODES table of cpu.rs, generated from the Rust source. */
typedef struct {
    AluOp alu;
    AddrMode mode;
} OpEntry;

static const OpEntry OPCODES[256] = {
      [0xc9] = { op_adc, mode_immediate },
      [0xc5] = { op_adc, mode_zero_page },
      [0xd5] = { op_adc, mode_zero_page_x },
      [0xcd] = { op_adc, mode_absolute },
      [0xdd] = { op_adc, mode_absolute_x },
      [0xd9] = { op_adc, mode_absolute_y },
      [0xc1] = { op_adc, mode_indirect_x },
      [0xd1] = { op_adc, mode_indirect_y },
      [0xe9] = { op_sbc, mode_immediate },
      [0xe5] = { op_sbc, mode_zero_page },
      [0xf5] = { op_sbc, mode_zero_page_x },
      [0xed] = { op_sbc, mode_absolute },
      [0xfd] = { op_sbc, mode_absolute_x },
      [0xf9] = { op_sbc, mode_absolute_y },
      [0xe1] = { op_sbc, mode_indirect_x },
      [0xf1] = { op_sbc, mode_indirect_y },
      [0x29] = { op_and, mode_immediate },
      [0x25] = { op_and, mode_zero_page },
      [0x35] = { op_and, mode_zero_page_x },
      [0x2d] = { op_and, mode_absolute },
      [0x3d] = { op_and, mode_absolute_x },
      [0x39] = { op_and, mode_absolute_y },
      [0x21] = { op_and, mode_indirect_x },
      [0x31] = { op_and, mode_indirect_y },
      [0x09] = { op_ora, mode_immediate },
      [0x05] = { op_ora, mode_zero_page },
      [0x15] = { op_ora, mode_zero_page_x },
      [0x0d] = { op_ora, mode_absolute },
      [0x1d] = { op_ora, mode_absolute_x },
      [0x19] = { op_ora, mode_absolute_y },
      [0x01] = { op_ora, mode_indirect_x },
      [0x11] = { op_ora, mode_indirect_y },
      [0x49] = { op_eor, mode_immediate },
      [0x45] = { op_eor, mode_zero_page },
      [0x55] = { op_eor, mode_zero_page_x },
      [0x4d] = { op_eor, mode_absolute },
      [0x5d] = { op_eor, mode_absolute_x },
      [0x59] = { op_eor, mode_absolute_y },
      [0x41] = { op_eor, mode_indirect_x },
      [0x51] = { op_eor, mode_indirect_y },
      [0x0a] = { op_asl, mode_implied_a },
      [0x06] = { op_asl, mode_zero_page },
      [0x16] = { op_asl, mode_zero_page_x },
      [0x0e] = { op_asl, mode_absolute },
      [0x1e] = { op_asl, mode_absolute_x },
      [0x4a] = { op_lsr, mode_implied_a },
      [0x46] = { op_lsr, mode_zero_page },
      [0x56] = { op_lsr, mode_zero_page_x },
      [0x4e] = { op_lsr, mode_absolute },
      [0x5e] = { op_lsr, mode_absolute_x },
      [0x2a] = { op_rol, mode_implied_a },
      [0x26] = { op_rol, mode_zero_page },
      [0x36] = { op_rol, mode_zero_page_x },
      [0x2e] = { op_rol, mode_absolute },
      [0x3e] = { op_rol, mode_absolute_x },
      [0x6a] = { op_ror, mode_implied_a },
      [0x66] = { op_ror, mode_zero_page },
      [0x76] = { op_ror, mode_zero_page_x },
      [0x6e] = { op_ror, mode_absolute },
      [0x7e] = { op_ror, mode_absolute_x },
      [0x24] = { op_bit, mode_zero_page },
      [0x2c] = { op_bit, mode_absolute },
      [0x69] = { op_cmp, mode_immediate },
      [0x65] = { op_cmp, mode_zero_page },
      [0x75] = { op_cmp, mode_zero_page_x },
      [0x6d] = { op_cmp, mode_absolute },
      [0x7d] = { op_cmp, mode_absolute_x },
      [0x79] = { op_cmp, mode_absolute_y },
      [0x61] = { op_cmp, mode_indirect_x },
      [0x71] = { op_cmp, mode_indirect_y },
      [0xc0] = { op_cpx, mode_immediate },
      [0xc4] = { op_cpx, mode_zero_page },
      [0xcc] = { op_cpx, mode_absolute },
      [0xe0] = { op_cpy, mode_immediate },
      [0xe4] = { op_cpy, mode_zero_page },
      [0xec] = { op_cpy, mode_absolute },
      [0xe6] = { op_inc, mode_zero_page },
      [0xf6] = { op_inc, mode_zero_page_x },
      [0xee] = { op_inc, mode_absolute },
      [0xfe] = { op_inc, mode_absolute_x },
      [0xc6] = { op_dec, mode_zero_page },
      [0xd6] = { op_dec, mode_zero_page_x },
      [0xce] = { op_dec, mode_absolute },
      [0xde] = { op_dec, mode_absolute_x },
      [0xe8] = { op_inc, mode_implied_x },
      [0xc8] = { op_inc, mode_implied_y },
      [0xca] = { op_dec, mode_implied_x },
      [0x88] = { op_dec, mode_implied_y },
      [0x10] = { op_bpl, mode_relative },
      [0xf0] = { op_bmi, mode_relative },
      [0x50] = { op_bvc, mode_relative },
      [0x70] = { op_bvs, mode_relative },
      [0x90] = { op_bcc, mode_relative },
      [0xb0] = { op_bcs, mode_relative },
      [0xd0] = { op_bne, mode_relative },
      [0x30] = { op_beq, mode_relative },
      [0xa9] = { op_lda, mode_immediate },
      [0xa5] = { op_lda, mode_zero_page },
      [0xb5] = { op_lda, mode_zero_page_x },
      [0xad] = { op_lda, mode_absolute },
      [0xbd] = { op_lda, mode_absolute_x },
      [0xb9] = { op_lda, mode_absolute_y },
      [0xa1] = { op_lda, mode_indirect_x },
      [0xb1] = { op_lda, mode_indirect_y },
      [0xa2] = { op_ldx, mode_immediate },
      [0xa6] = { op_ldx, mode_zero_page },
      [0xb6] = { op_ldx, mode_zero_page_y },
      [0xae] = { op_ldx, mode_absolute },
      [0xbe] = { op_ldx, mode_absolute_y },
      [0xa0] = { op_ldy, mode_immediate },
      [0xa4] = { op_ldy, mode_zero_page },
      [0xb4] = { op_ldy, mode_zero_page_x },
      [0xac] = { op_ldy, mode_absolute },
      [0xbc] = { op_ldy, mode_absolute_x },
      [0x85] = { op_sta, mode_zero_page },
      [0x95] = { op_sta, mode_zero_page_x },
      [0x8d] = { op_sta, mode_absolute },
      [0x9d] = { op_sta, mode_absolute_x },
      [0x99] = { op_sta, mode_absolute_y },
      [0x81] = { op_sta, mode_indirect_x },
      [0x91] = { op_sta, mode_indirect_y },
      [0x86] = { op_stx, mode_zero_page },
      [0x96] = { op_stx, mode_zero_page_y },
      [0x8e] = { op_stx, mode_absolute },
      [0x84] = { op_sty, mode_zero_page },
      [0x94] = { op_sty, mode_zero_page_x },
      [0x8c] = { op_sty, mode_absolute },
      [0x20] = { op_jsr, mode_absolute },
};

static void cpu_tick(Cpu *cpu, Chipset *mem) {
    uint8_t op = chip_read(mem, cpu->pc);
    cpu->pc = (uint16_t)(cpu->pc + 1);

    const OpEntry *e = &OPCODES[op];
    if (e->alu)
        e->alu(cpu, mem, e->mode);
    else
        manual(cpu, mem, op);

    if (cpu->nmi_waiting || (cpu->irq_waiting && !cpu->irq_disable)) {
        uint16_t addr;
        if (cpu->nmi_waiting) {
            cpu->nmi_waiting = false;
            addr = chip_read16(mem, 0xFFFA);
        } else {
            cpu->irq_waiting = false;
            addr = chip_read16(mem, 0xFFFE);
        }

        cpu->count += 7;
        uint16_t pc = cpu->pc;
        push16(cpu, mem, pc);

        bool interrupt = cpu->interrupt;
        cpu->interrupt = false;
        uint8_t p = cpu_get_p(cpu);
        push(cpu, mem, p);
        cpu->interrupt = interrupt;

        cpu->pc = addr;
        cpu->irq_disable = true;
    }
}

/* ----------------------------------------------------------- Console driving */

static void console_tick(Cpu *cpu, Chipset *c) {
    const uint32_t frame_time = 1789773 / 60;
    while (cpu->count < frame_time) {
        if (c->ppu_dma_requested) {
            c->ppu_dma_requested = false;
            ppudma(&c->ppu, c, c->ppu_dma_val, cpu);
        }

        if (c->nppu_writes > 0) {
            for (size_t i = 0; i < c->nppu_writes; i++)
                ppu_write_main(&c->ppu, &c->mapper, c->ppu_writes_requested[i].addr,
                               c->ppu_writes_requested[i].val, cpu);
            c->nppu_writes = 0;
        }

        cpu_tick(cpu, c);
        ppu_tick(&c->ppu, cpu, &c->mapper);
    }

    cpu->count -= frame_time;
}

/* ------------------------------------------------------------- Emu API */

Emu *emu_load_rom(const uint8_t *file, size_t len) {
    if (len < 16)
        return NULL;

    /* binloader::load_file */
    size_t prg_size = (size_t)file[0] * 16384;
    size_t chr_size = (size_t)file[1] * 8192;
    size_t prg_ram_size = 8192;
    bool horiz_mirroring = true;
    if (16 + prg_size + chr_size > len)
        return NULL;
    const uint8_t *prg = file + 16;
    const uint8_t *chr = file + 16 + prg_size;

    Emu *e = calloc(1, sizeof *e);
    if (!e)
        fail("out of memory");

    /* Console::new */
    static const uint8_t zero_chr[8 * 1024];
    if (chr_size == 0) {
        chr = zero_chr;
        chr_size = sizeof zero_chr;
    }
    Chipset *c = &e->chip;
    mapper_init(&c->mapper, prg, prg_size, prg_ram_size, chr, chr_size);

    e->cpu.a = 0;
    e->cpu.x = 0;
    e->cpu.y = 0;
    e->cpu.s = 0xFD;
    e->cpu.pc = memory_read16(c, 0xFFFC);
    e->cpu.negative = true;
    e->cpu.overflow = false;
    e->cpu.interrupt = true;
    e->cpu.irq_disable = false;
    e->cpu.carry = false;
    e->cpu.zero = false;
    e->cpu.count = 0;
    e->cpu.nmi_waiting = false;
    e->cpu.irq_waiting = false;
    e->cpu.decimal = false;

    /* Ppu::new(horiz_mirroring) and make_canvas are all-zero except for the flag. */
    c->ppu.horiz_mapping = horiz_mirroring;
    return e;
}

void emu_free(Emu *e) {
    if (!e)
        return;
    mapper_free(&e->chip.mapper);
    ppu_states_clear(&e->chip.ppu);
    free(e->chip.ppu.states);
    free(e);
}

/* benchmark::bench_tick */
const uint8_t *emu_tick(Emu *e, const char *input) {
    Chipset *c = &e->chip;

    c->controller1.up = false;
    c->controller1.left = false;
    c->controller1.down = false;
    c->controller1.right = false;
    c->controller1.a = false;
    c->controller1.b = false;
    c->controller1.start = false;
    c->controller1.select = false;

    for (const char *ch = input; *ch; ch++) {
        switch (*ch) {
        case 'u': c->controller1.up = true; break;
        case 'l': c->controller1.left = true; break;
        case 'd': c->controller1.down = true; break;
        case 'r': c->controller1.right = true; break;
        case 'a': c->controller1.a = true; break;
        case 'b': c->controller1.b = true; break;
        case 't': c->controller1.start = true; break;
        case 'e': c->controller1.select = true; break;
        default: break;
        }
    }

    console_tick(&e->cpu, c);

    /* Console::prepare_draw: render, then copy into the canvas. */
    ppu_prepare_draw(&c->ppu, &c->mapper);
    for (int y = 0; y < PPU_HEIGHT; y++)
        for (int x = 0; x < PPU_WIDTH; x++) {
            int sx = x * PPU_WIDTH / PPU_WIDTH;
            int sy = y * PPU_HEIGHT / PPU_HEIGHT;
            memcpy(e->canvas[y][x], c->ppu.output_canvas[sy][sx], 4);
        }

    /* app.video = app.canvas.as_raw().to_vec() */
    memcpy(e->video, e->canvas, sizeof e->video);
    return e->video;
}
