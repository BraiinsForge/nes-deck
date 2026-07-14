#include <cassert>
#include <cstdio>

#include "../src/zx_keyboard.h"

int main(void) {
  assert(zx_linux_keycode(RETROK_SPACE) == KEY_SPACE);
  assert(zx_linux_keycode(RETROK_RETURN) == KEY_ENTER);
  assert(zx_linux_keycode(RETROK_BACKSPACE) == KEY_BACKSPACE);
  assert(zx_linux_keycode(RETROK_0) == KEY_0);
  assert(zx_linux_keycode(RETROK_9) == KEY_9);
  assert(zx_linux_keycode(RETROK_a) == KEY_A);
  assert(zx_linux_keycode(RETROK_z) == KEY_Z);
  assert(zx_linux_keycode(RETROK_LSHIFT) == KEY_LEFTSHIFT);
  assert(zx_linux_keycode(RETROK_RCTRL) == KEY_RIGHTCTRL);
  assert(zx_linux_keycode(RETROK_UP) == KEY_UP);
  assert(zx_linux_keycode(RETROK_LEFT) == KEY_LEFT);
  assert(zx_linux_keycode(RETROK_F1) == KEY_RESERVED);
  std::puts("zx_keyboard_test: OK");
  return 0;
}
