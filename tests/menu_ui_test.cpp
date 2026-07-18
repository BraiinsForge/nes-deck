#include <cassert>
#include <iostream>

#include "../src/menu_ui.h"

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

  assert(display_ascii("ASCII") == "ASCII");
  assert(display_ascii("\xc4\x8c") == "?");
  assert(text_width("AB", 2) == 22);
  assert(fit_text_scale("ABCDE", 29, 3, 1) == 1);
  assert(fit_text_width("ABCDEFGHIJ", 29, 1) == "AB...");

  draw_text(&canvas, 100, 100, "A", 2, foreground);
  assert(canvas[100 * kLogicalWidth + 104] == foreground);

  std::cout << "menu_ui_test: OK\n";
  return 0;
}
