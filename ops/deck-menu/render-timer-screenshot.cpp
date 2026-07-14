#include <cstdio>
#include <cstring>

#include <png.h>

#define main ten_seconds_deck_embedded_main
#include "../../src/ten_seconds_deck.cpp"
#undef main

namespace {

bool write_timer_png(const char *path, const Canvas &source) {
  DeckScaledLayout layout;
  if (!path || source.size() !=
                   static_cast<size_t>(kCanvasWidth * kCanvasHeight) ||
      !DeckComputeScaledLayout(kCanvasWidth, kCanvasHeight, &layout)) {
    return false;
  }

  Canvas screen(static_cast<size_t>(kLogicalWidth * kLogicalHeight), 0);
  for (int source_y = 0; source_y < kCanvasHeight; ++source_y) {
    for (int source_x = 0; source_x < kCanvasWidth; ++source_x) {
      const uint16_t color =
          source[static_cast<size_t>(source_y) * kCanvasWidth + source_x];
      for (int y = 0; y < layout.scale; ++y) {
        for (int x = 0; x < layout.scale; ++x) {
          const int destination_x = layout.x + source_x * layout.scale + x;
          const int destination_y = layout.y + source_y * layout.scale + y;
          screen[static_cast<size_t>(destination_y) * kLogicalWidth +
                 destination_x] = color;
        }
      }
    }
  }

  std::vector<png_byte> rgb(screen.size() * 3);
  for (size_t index = 0; index < screen.size(); ++index) {
    const uint16_t pixel = screen[index];
    const unsigned int red = (pixel >> 11) & 0x1f;
    const unsigned int green = (pixel >> 5) & 0x3f;
    const unsigned int blue = pixel & 0x1f;
    rgb[index * 3] = static_cast<png_byte>((red << 3) | (red >> 2));
    rgb[index * 3 + 1] =
        static_cast<png_byte>((green << 2) | (green >> 4));
    rgb[index * 3 + 2] =
        static_cast<png_byte>((blue << 3) | (blue >> 2));
  }

  png_image image;
  std::memset(&image, 0, sizeof(image));
  image.version = PNG_IMAGE_VERSION;
  image.width = kLogicalWidth;
  image.height = kLogicalHeight;
  image.format = PNG_FORMAT_RGB;
  if (!png_image_write_to_file(&image, path, 0, &rgb[0], 0, NULL)) {
    std::fprintf(stderr, "cannot write %s: %s\n", path, image.message);
    return false;
  }
  return true;
}

} // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "Usage: %s OUTPUT.png\n", argv[0]);
    return 2;
  }
  Canvas timer;
  render_timer(&timer, TimerStopped, 1003);
  return write_timer_png(argv[1], timer) ? 0 : 1;
}
