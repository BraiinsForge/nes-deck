#include <cassert>
#include <cstdlib>
#include <iostream>
#include <string>

#include "../src/deck_runtime.h"

int main() {
  DeckScaledLayout layout;
  assert(DeckComputeScaledLayout(160, 144, &layout));
  assert(layout.scale == 3);
  assert(layout.x == 400 && layout.y == 24);
  assert(layout.width == 480 && layout.height == 432);

  assert(DeckComputeScaledLayout(256, 224, &layout));
  assert(layout.scale == 2);
  assert(layout.x == 384 && layout.y == 16);
  assert(layout.width == 512 && layout.height == 448);

  assert(DeckComputeScaledLayout(288, 216, &layout));
  assert(layout.scale == 2);
  assert(layout.x == 352 && layout.y == 24);
  assert(layout.width == 576 && layout.height == 432);

  assert(DeckComputeScaledLayout(64, 32, &layout));
  assert(layout.scale == 14);
  assert(layout.x == 192 && layout.y == 16);
  assert(layout.width == 896 && layout.height == 448);

  assert(DeckComputeScaledLayout(128, 64, &layout));
  assert(layout.scale == 7);
  assert(layout.x == 192 && layout.y == 16);
  assert(!DeckComputeScaledLayout(2000, 1000, &layout));

  assert(DeckRgb888To565(0xff0000) == 0xf800);
  assert(DeckRgb888To565(0x00ff00) == 0x07e0);
  assert(DeckRgb888To565(0x0000ff) == 0x001f);
  assert(DeckRgb888To565(0xffffff) == 0xffff);

  assert(DeckAudioOutputRate(48000, 48000) == 47328);
  assert(DeckAudioOutputRate(32768, 32000) == 32000);
  assert(DeckAudioOutputRate(44100, 44100) == 44100);

  unsetenv("RETRO_DECK_EXIT_HINT");
  assert(!DeckExitHintRequested());
  setenv("RETRO_DECK_EXIT_HINT", "1", 1);
  assert(DeckExitHintRequested());
  setenv("RETRO_DECK_EXIT_HINT", "0", 1);
  assert(!DeckExitHintRequested());
  setenv("RETRO_DECK_EXIT_HINT", "invalid", 1);
  assert(!DeckExitHintRequested());
  unsetenv("RETRO_DECK_EXIT_HINT");

  std::vector<uint16_t> exit_hint(600 * 1280, 0x1234);
  DeckDrawExitHintRgb565(&exit_hint[0], 600);
  assert(exit_hint[(1279 - 20) * 600 + 20] == 0xffff);
  assert(exit_hint[(1279 - 36) * 600 + 36] == 0xffff);
  assert(exit_hint[(1279 - 18) * 600 + 18] == 0x0000);
  assert(exit_hint[(1279 - 100) * 600 + 100] == 0x1234);

  unsigned int volume = 0;
  std::string error;
  unsetenv("RETRO_DECK_VOLUME_PERCENT");
  unsetenv("INFONES_VOLUME_PERCENT");
  assert(DeckReadVolumePercent(&volume, &error) && volume == 42);
  setenv("INFONES_VOLUME_PERCENT", "31", 1);
  assert(DeckReadVolumePercent(&volume, &error) && volume == 31);
  setenv("RETRO_DECK_VOLUME_PERCENT", "57", 1);
  assert(DeckReadVolumePercent(&volume, &error) && volume == 57);
  setenv("RETRO_DECK_VOLUME_PERCENT", "101", 1);
  assert(!DeckReadVolumePercent(&volume, &error));
  unsetenv("RETRO_DECK_VOLUME_PERCENT");
  unsetenv("INFONES_VOLUME_PERCENT");

  std::cout << "deck_runtime_test: OK\n";
  return 0;
}
