#ifndef RETRO_DECK_CHIP8_CORE_H
#define RETRO_DECK_CHIP8_CORE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct Chip8Core Chip8Core;

typedef struct {
  int tickrate;
  int shift_quirk;
  int load_store_quirk;
  int jump_quirk;
  int logic_quirk;
  int clip_quirk;
  int vblank_quirk;
  uint32_t colors[4];
} Chip8CoreOptions;

void Chip8CoreDefaultOptions(Chip8CoreOptions *options);
Chip8Core *Chip8CoreCreate(const uint8_t *rom, size_t size,
                           const Chip8CoreOptions *options);
void Chip8CoreDestroy(Chip8Core *core);
void Chip8CoreSetKey(Chip8Core *core, unsigned int key, int pressed);
int Chip8CoreRunFrame(Chip8Core *core);
const uint8_t *Chip8CorePixels(const Chip8Core *core, unsigned int *width,
                               unsigned int *height, size_t *pitch);
const uint32_t *Chip8CorePalette(const Chip8Core *core);
int Chip8CoreHalted(const Chip8Core *core);
const char *Chip8CoreHaltMessage(const Chip8Core *core);

#ifdef __cplusplus
}
#endif

#endif /* RETRO_DECK_CHIP8_CORE_H */
