#include <cassert>
#include <cstdio>

#define RETRO_DECK_ZX 1
#include "../src/joypad_input.cpp"

int main(void) {
  assert(keycode_to_pad(KEY_SPACE) == 0);
  assert(keycode_to_pad(KEY_ENTER) == 0);
  assert(keycode_to_pad(KEY_LEFT) == 0);

  update_keyboard_key(KEY_SPACE, true);
  assert(GetKeyboardInput(KEY_SPACE) == 1);
  assert(GetJoypadInput(0) == 0);
  update_keyboard_key(KEY_SPACE, false);
  assert(GetKeyboardInput(KEY_SPACE) == 0);

  GamepadDevice first;
  first.x_info.minimum = 0;
  first.x_info.maximum = 255;
  first.y_info.minimum = 0;
  first.y_info.maximum = 255;
  first.x_value = 127;
  first.y_value = 127;
  first.raw_buttons = 1u << (BTN_THUMB2 - BTN_TRIGGER);
  gamepads[0].state = gamepad_state(first);
  publish_states();
  assert(GetJoypadInput(0) == PAD_A);

  std::puts("joypad_input_zx_test: OK");
  return 0;
}
