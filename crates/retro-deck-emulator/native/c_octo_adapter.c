#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

/* Exact upstream source, checksummed under vendor/emulators/c-octo. */
#include "octo_emulator.h"

#define RD_QUIRK_SHIFT (1U << 0)
#define RD_QUIRK_LOAD_STORE (1U << 1)
#define RD_QUIRK_JUMP (1U << 2)
#define RD_QUIRK_LOGIC (1U << 3)
#define RD_QUIRK_CLIP (1U << 4)
#define RD_QUIRK_VBLANK (1U << 5)
#define RD_MAXIMUM_ROM_BYTES 65024U
#define RD_MINIMUM_TICKRATE 1U
#define RD_MAXIMUM_TICKRATE 50000U

typedef struct {
  uint32_t instructions_per_frame;
  uint8_t quirks;
  uint8_t reserved[3];
} RdCOctoOptions;

typedef struct {
  octo_emulator emulator;
} RdCOcto;

RdCOcto *rd_c_octo_create(const uint8_t *rom, size_t size,
                          const RdCOctoOptions *options) {
  if (rom == NULL || options == NULL || size == 0 ||
      size > RD_MAXIMUM_ROM_BYTES ||
      options->instructions_per_frame < RD_MINIMUM_TICKRATE ||
      options->instructions_per_frame > RD_MAXIMUM_TICKRATE) {
    return NULL;
  }

  RdCOcto *core = (RdCOcto *)calloc(1, sizeof(*core));
  if (core == NULL) {
    return NULL;
  }
  octo_options selected;
  octo_default_options(&selected);
  selected.tickrate = (int)options->instructions_per_frame;
  selected.max_rom = size <= 3584U ? 3584 : (int)RD_MAXIMUM_ROM_BYTES;
  selected.q_shift = (options->quirks & RD_QUIRK_SHIFT) != 0;
  selected.q_loadstore = (options->quirks & RD_QUIRK_LOAD_STORE) != 0;
  selected.q_jump0 = (options->quirks & RD_QUIRK_JUMP) != 0;
  selected.q_logic = (options->quirks & RD_QUIRK_LOGIC) != 0;
  selected.q_clip = (options->quirks & RD_QUIRK_CLIP) != 0;
  selected.q_vblank = (options->quirks & RD_QUIRK_VBLANK) != 0;
  octo_emulator_init(&core->emulator, (char *)(void *)rom, size, &selected,
                     NULL);
  return core;
}

void rd_c_octo_destroy(RdCOcto *core) { free(core); }

void rd_c_octo_set_keys(RdCOcto *core, uint16_t keys) {
  if (core == NULL) {
    return;
  }
  for (unsigned int key = 0; key < 16U; ++key) {
    const int was_pressed = core->emulator.keys[key] != 0;
    const int pressed = (keys & (uint16_t)(1U << key)) != 0;
    core->emulator.keys[key] = (char)pressed;
    if (was_pressed && !pressed && core->emulator.wait) {
      core->emulator.v[(unsigned int)core->emulator.wait_reg] = (uint8_t)key;
      core->emulator.wait = 0;
    }
  }
}

int rd_c_octo_run_frame(RdCOcto *core) {
  if (core == NULL || core->emulator.halt) {
    return 0;
  }
  for (int instruction = 0;
       instruction < core->emulator.options.tickrate && !core->emulator.halt;
       ++instruction) {
    if (core->emulator.options.q_vblank &&
        (core->emulator.ram[core->emulator.pc] & 0xf0U) == 0xd0U) {
      break;
    }
    octo_emulator_instruction(&core->emulator);
  }
  if (core->emulator.dt > 0) {
    --core->emulator.dt;
  }
  const int sound = core->emulator.st > 0;
  if (core->emulator.st > 0) {
    --core->emulator.st;
  }
  return sound;
}

const uint8_t *rd_c_octo_pixels(const RdCOcto *core, uint32_t *width,
                                uint32_t *height) {
  if (core == NULL || width == NULL || height == NULL) {
    return NULL;
  }
  *width = core->emulator.hires ? 128U : 64U;
  *height = core->emulator.hires ? 64U : 32U;
  return core->emulator.px;
}

int rd_c_octo_halted(const RdCOcto *core) {
  return core == NULL || core->emulator.halt != 0;
}

const char *rd_c_octo_halt_message(const RdCOcto *core) {
  return core == NULL ? "emulator is unavailable" : core->emulator.halt_message;
}
