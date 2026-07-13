#include <assert.h>
#include <stdio.h>
#include <stdlib.h>

#include "../src/chip8_core.h"

static void smoke_game(const char *path, int tickrate) {
  FILE *file = fopen(path, "rb");
  assert(file != NULL);
  assert(fseek(file, 0, SEEK_END) == 0);
  long length = ftell(file);
  assert(length > 0 && length <= 65024);
  rewind(file);
  uint8_t *rom = (uint8_t *)malloc((size_t)length);
  assert(rom != NULL);
  assert(fread(rom, 1, (size_t)length, file) == (size_t)length);
  assert(fclose(file) == 0);

  Chip8CoreOptions options;
  Chip8CoreDefaultOptions(&options);
  options.tickrate = tickrate;
  Chip8Core *core = Chip8CoreCreate(rom, (size_t)length, &options);
  assert(core != NULL);
  for (int frame = 0; frame < 600; ++frame) {
    Chip8CoreRunFrame(core);
    assert(!Chip8CoreHalted(core));
  }
  Chip8CoreDestroy(core);
  free(rom);
}

int main(int argc, char **argv) {
  /* Clear, draw the built-in zero glyph at 0,0, then exit via SCHIP 00FD. */
  const uint8_t rom[] = {
      0x00, 0xe0, 0x60, 0x00, 0x61, 0x00,
      0xa0, 0x00, 0xd0, 0x15, 0x00, 0xfd,
  };
  Chip8CoreOptions options;
  Chip8CoreDefaultOptions(&options);
  options.tickrate = 20;
  Chip8Core *core = Chip8CoreCreate(rom, sizeof(rom), &options);
  assert(core != NULL);
  assert(Chip8CoreRunFrame(core) == 0);
  assert(Chip8CoreHalted(core));

  unsigned int width = 0;
  unsigned int height = 0;
  size_t pitch = 0;
  const uint8_t *pixels = Chip8CorePixels(core, &width, &height, &pitch);
  assert(pixels != NULL);
  assert(width == 64 && height == 32 && pitch == 64);
  unsigned int lit = 0;
  for (unsigned int y = 0; y < height; ++y)
    for (unsigned int x = 0; x < width; ++x)
      lit += pixels[y * pitch + x] != 0;
  assert(lit > 0);
  assert(Chip8CorePalette(core)[0] == 0x000000);
  assert(Chip8CorePalette(core)[1] == 0xffcc00);
  Chip8CoreDestroy(core);

  assert(Chip8CoreCreate(rom, 0, &options) == NULL);
  if (argc == 3) {
    smoke_game(argv[1], 15);
    smoke_game(argv[2], 20);
  }
  puts("chip8_core_test: OK");
  return 0;
}
