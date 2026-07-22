#include <cassert>
#include <iostream>

#include "../src/menu_ui.h"

namespace {

uint16_t rgb565(uint32_t color) {
  const uint32_t red = (color >> 16) & 0xff;
  const uint32_t green = (color >> 8) & 0xff;
  const uint32_t blue = color & 0xff;
  return static_cast<uint16_t>(((red & 0xf8) << 8) |
                               ((green & 0xfc) << 3) | (blue >> 3));
}

uint64_t canvas_hash(const Canvas &canvas) {
  uint64_t hash = UINT64_C(0xcbf29ce484222325);
  for (uint16_t pixel : canvas) {
    hash ^= pixel & 0xff;
    hash *= UINT64_C(0x100000001b3);
    hash ^= pixel >> 8;
    hash *= UINT64_C(0x100000001b3);
  }
  return hash;
}

void draw_pixel_panel(Canvas *canvas, const Rect &rect, uint16_t fill,
                      uint16_t border, int thickness) {
  fill_pixel_cut_rect(canvas, rect, thickness, border);
  fill_pixel_cut_rect(
      canvas,
      Rect{rect.x + thickness, rect.y + thickness,
           rect.width - thickness * 2, rect.height - thickness * 2},
      thickness, fill);
}

} // namespace

int main() {
  const uint16_t background = 0x0000;
  const uint16_t foreground = 0xffff;
  Canvas canvas(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                background);

  const Rect target{10, 20, 30, 40};
  assert(target.contains(10, 20));
  assert(target.contains(39, 59));
  assert(!target.contains(40, 59));

  fill_rect(&canvas, Rect{-4, -3, 8, 6}, foreground);
  assert(canvas[0] == foreground);
  assert(canvas[2 * kLogicalWidth + 3] == foreground);
  assert(canvas[3 * kLogicalWidth + 3] == background);

  stroke_rect(&canvas, target, 2, foreground);
  assert(canvas[20 * kLogicalWidth + 10] == foreground);
  assert(canvas[22 * kLogicalWidth + 12] == background);

  assert(text_width("AB", 2) == 22);
  assert(fit_text_scale("ABCDE", 29, 3, 1) == 1);
  assert(fit_text_width("ABCDEFGHIJ", 29, 1) == "AB...");

  draw_text(&canvas, 100, 100, "A", 2, foreground);
  assert(canvas[100 * kLogicalWidth + 104] == foreground);

  Canvas fixture(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                 rgb565(0x000000));
  const Rect panel{100, 100, 200, 80};
  const Rect outline{340, 100, 180, 80};
  draw_pixel_panel(&fixture, panel, rgb565(0x121212), rgb565(0xfe6c27), 4);
  stroke_rect(&fixture, outline, 4, rgb565(0xeeeeee));
  draw_centered_text(&fixture, panel, "RETRO", 2, rgb565(0xeeeeee));
  draw_centered_text(&fixture, outline,
                     fit_text_width("ABCDEFGHIJ", 100, 2), 2,
                     rgb565(0xeeeeee));
  draw_text(&fixture, 10, 10, std::string("A") + "\xc4\x8c", 1,
            rgb565(0xffffaf));
  // Shared with native fbdev::tests::matches_cpp_ui_fixture.
  assert(canvas_hash(fixture) == UINT64_C(0x414079453e1344d5));

  std::cout << "menu_ui_test: OK\n";
  return 0;
}
