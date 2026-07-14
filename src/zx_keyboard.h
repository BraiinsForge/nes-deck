#ifndef RETRO_DECK_ZX_KEYBOARD_H
#define RETRO_DECK_ZX_KEYBOARD_H

#include <libretro.h>
#include <linux/input.h>

inline unsigned int zx_linux_keycode(unsigned int retro_key) {
  static const unsigned int digit_keys[] = {
      KEY_0, KEY_1, KEY_2, KEY_3, KEY_4,
      KEY_5, KEY_6, KEY_7, KEY_8, KEY_9,
  };
  static const unsigned int letter_keys[] = {
      KEY_A, KEY_B, KEY_C, KEY_D, KEY_E, KEY_F, KEY_G,
      KEY_H, KEY_I, KEY_J, KEY_K, KEY_L, KEY_M, KEY_N,
      KEY_O, KEY_P, KEY_Q, KEY_R, KEY_S, KEY_T, KEY_U,
      KEY_V, KEY_W, KEY_X, KEY_Y, KEY_Z,
  };
  if (retro_key >= RETROK_0 && retro_key <= RETROK_9)
    return digit_keys[retro_key - RETROK_0];
  if (retro_key >= RETROK_a && retro_key <= RETROK_z)
    return letter_keys[retro_key - RETROK_a];
  switch (retro_key) {
  case RETROK_RETURN:
    return KEY_ENTER;
  case RETROK_SPACE:
    return KEY_SPACE;
  case RETROK_BACKSPACE:
    return KEY_BACKSPACE;
  case RETROK_LSHIFT:
    return KEY_LEFTSHIFT;
  case RETROK_RSHIFT:
    return KEY_RIGHTSHIFT;
  case RETROK_LCTRL:
    return KEY_LEFTCTRL;
  case RETROK_RCTRL:
    return KEY_RIGHTCTRL;
  case RETROK_LALT:
    return KEY_LEFTALT;
  case RETROK_RALT:
    return KEY_RIGHTALT;
  case RETROK_LSUPER:
    return KEY_LEFTMETA;
  case RETROK_RSUPER:
    return KEY_RIGHTMETA;
  case RETROK_UP:
    return KEY_UP;
  case RETROK_DOWN:
    return KEY_DOWN;
  case RETROK_LEFT:
    return KEY_LEFT;
  case RETROK_RIGHT:
    return KEY_RIGHT;
  default:
    return KEY_RESERVED;
  }
}

#endif
