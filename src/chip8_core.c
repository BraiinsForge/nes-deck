#include "chip8_core.h"

#include <stdlib.h>
#include <string.h>

/* Supplied by the pinned c-octo source input at build time. */
#include "octo_emulator.h"

struct Chip8Core {
  octo_emulator emulator;
  uint32_t palette[4];
};

void Chip8CoreDefaultOptions(Chip8CoreOptions *options) {
  if (!options)
    return;
  memset(options, 0, sizeof(*options));
  options->tickrate = 20;
  options->colors[0] = 0x000000;
  options->colors[1] = 0xffcc00;
  options->colors[2] = 0xff6600;
  options->colors[3] = 0x662200;
}

Chip8Core *Chip8CoreCreate(const uint8_t *rom, size_t size,
                           const Chip8CoreOptions *options) {
  if (!rom || size == 0 || size > 65024)
    return NULL;
  Chip8CoreOptions selected;
  Chip8CoreDefaultOptions(&selected);
  if (options)
    selected = *options;
  if (selected.tickrate < 1 || selected.tickrate > 50000)
    return NULL;

  Chip8Core *core = (Chip8Core *)calloc(1, sizeof(*core));
  if (!core)
    return NULL;
  octo_options octo;
  octo_default_options(&octo);
  octo.tickrate = selected.tickrate;
  octo.max_rom = size <= 3584 ? 3584 : 65024;
  octo.q_shift = selected.shift_quirk != 0;
  octo.q_loadstore = selected.load_store_quirk != 0;
  octo.q_jump0 = selected.jump_quirk != 0;
  octo.q_logic = selected.logic_quirk != 0;
  octo.q_clip = selected.clip_quirk != 0;
  octo.q_vblank = selected.vblank_quirk != 0;
  for (int i = 0; i < 4; ++i) {
    core->palette[i] = selected.colors[i] & 0xffffff;
    octo.colors[i] = (int)(0xff000000U | core->palette[i]);
  }
  octo_emulator_init(&core->emulator, (char *)rom, size, &octo, NULL);
  return core;
}

void Chip8CoreDestroy(Chip8Core *core) { free(core); }

void Chip8CoreSetKey(Chip8Core *core, unsigned int key, int pressed) {
  if (!core || key >= 16)
    return;
  const int was_pressed = core->emulator.keys[key] != 0;
  core->emulator.keys[key] = pressed != 0;
  if (was_pressed && !pressed && core->emulator.wait) {
    core->emulator.v[(int)core->emulator.wait_reg] = (uint8_t)key;
    core->emulator.wait = 0;
  }
}

int Chip8CoreRunFrame(Chip8Core *core) {
  if (!core || core->emulator.halt)
    return 0;
  for (int i = 0; i < core->emulator.options.tickrate &&
                  !core->emulator.halt;
       ++i) {
    if (core->emulator.options.q_vblank &&
        (core->emulator.ram[core->emulator.pc] & 0xf0) == 0xd0)
      i = core->emulator.options.tickrate;
    octo_emulator_instruction(&core->emulator);
  }
  if (core->emulator.dt > 0)
    --core->emulator.dt;
  const int sound = core->emulator.st > 0;
  if (core->emulator.st > 0)
    --core->emulator.st;
  return sound;
}

const uint8_t *Chip8CorePixels(const Chip8Core *core, unsigned int *width,
                               unsigned int *height, size_t *pitch) {
  if (!core)
    return NULL;
  if (width)
    *width = core->emulator.hires ? 128 : 64;
  if (height)
    *height = core->emulator.hires ? 64 : 32;
  if (pitch)
    *pitch = core->emulator.hires ? 128 : 64;
  return core->emulator.px;
}

const uint32_t *Chip8CorePalette(const Chip8Core *core) {
  return core ? core->palette : NULL;
}

int Chip8CoreHalted(const Chip8Core *core) {
  return core ? core->emulator.halt != 0 : 1;
}

const char *Chip8CoreHaltMessage(const Chip8Core *core) {
  return core ? core->emulator.halt_message : "emulator is unavailable";
}
