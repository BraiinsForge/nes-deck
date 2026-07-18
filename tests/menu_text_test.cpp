#include <cassert>
#include <climits>
#include <iostream>
#include <string>

#include "../src/menu_text.h"

int main() {
  assert(is_absolute_path("/mnt/data/roms/nes/game.nes"));
  assert(!is_absolute_path("mnt/data/roms/nes/game.nes"));
  assert(!is_absolute_path(""));
  assert(!is_absolute_path(std::string(PATH_MAX, 'x')));

  assert(trim_ascii_space("  RETRO DECK\t") == "RETRO DECK");
  assert(trim_ascii_space("NO CHANGE") == "NO CHANGE");
  assert(trim_ascii_space(" \n\t") == "");

  assert(valid_utf8_text("ASCII", 5, false));
  assert(valid_utf8_text("\xc4\x8c", 1, false));
  assert(valid_utf8_text("", 0, true));
  assert(!valid_utf8_text("", 1, false));
  assert(!valid_utf8_text("TOO LONG", 7, false));
  assert(!valid_utf8_text("LINE\nBREAK", 32, false));
  assert(!valid_utf8_text("\xc0\xaf", 1, false));
  assert(!valid_utf8_text("\xed\xa0\x80", 1, false));
  assert(!valid_utf8_text("\xf4\x90\x80\x80", 1, false));
  assert(!valid_utf8_text("\xe2\x82", 1, false));

  assert(display_ascii("ASCII") == "ASCII");
  assert(display_ascii("\xc4\x8c") == "?");
  assert(display_ascii("A\xc4\x8c B") == "A? B");

  std::cout << "menu_text_test: OK\n";
  return 0;
}
