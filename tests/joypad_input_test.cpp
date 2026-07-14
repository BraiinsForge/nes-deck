#include <cassert>
#include <cstdio>

#include "../src/joypad_input.cpp"

static void test_keyboard_mapping(void) {
  assert(keycode_to_pad(0x67) == PAD_UP);
  assert(keycode_to_pad(0x6c) == PAD_DOWN);
  assert(keycode_to_pad(0x69) == PAD_LEFT);
  assert(keycode_to_pad(0x6a) == PAD_RIGHT);
  assert(keycode_to_pad(0x39) == PAD_A);
  assert(keycode_to_pad(0x2c) == PAD_A);
  assert(keycode_to_pad(0x2a) == PAD_B);
  assert(keycode_to_pad(0x36) == PAD_B);
  assert(keycode_to_pad(0x2d) == PAD_B);
  assert(keycode_to_pad(0x1d) == PAD_SELECT);
  assert(keycode_to_pad(0x61) == PAD_SELECT);
  assert(keycode_to_pad(0x1c) == PAD_START);
}

static void test_published_thegamepad_mapping(void) {
  assert(gamepad_key_to_pad(BTN_THUMB2) == PAD_A);  // A / SDL b2
  assert(gamepad_key_to_pad(BTN_TOP) == PAD_A);     // X / SDL b3
  assert(gamepad_key_to_pad(BTN_THUMB) == PAD_B);   // B / SDL b1
  assert(gamepad_key_to_pad(BTN_TRIGGER) == PAD_B); // Y / SDL b0
  assert(gamepad_key_to_pad(BTN_BASE) == PAD_SELECT);
  assert(gamepad_key_to_pad(BTN_BASE2) == PAD_START);
  assert(gamepad_key_to_pad(BTN_TOP2) == PAD_L);
  assert(gamepad_key_to_pad(BTN_PINKIE) == PAD_R);
}

static void test_digital_axes(void) {
  assert(axis_to_pad(0, 0, 255, PAD_LEFT, PAD_RIGHT) == PAD_LEFT);
  assert(axis_to_pad(127, 0, 255, PAD_LEFT, PAD_RIGHT) == 0);
  assert(axis_to_pad(255, 0, 255, PAD_LEFT, PAD_RIGHT) == PAD_RIGHT);
  assert(axis_to_pad(-32767, -32767, 32767, PAD_UP, PAD_DOWN) == PAD_UP);
  assert(axis_to_pad(0, -32767, 32767, PAD_UP, PAD_DOWN) == 0);
  assert(axis_to_pad(32767, -32767, 32767, PAD_UP, PAD_DOWN) == PAD_DOWN);
}

static void test_two_independent_players(void) {
  GamepadDevice first;
  first.x_info.minimum = 0;
  first.x_info.maximum = 255;
  first.y_info.minimum = 0;
  first.y_info.maximum = 255;
  first.x_value = 0;
  first.y_value = 127;
  first.raw_buttons = 1u << (BTN_THUMB2 - BTN_TRIGGER);
  assert(gamepad_state(first) == (PAD_LEFT | PAD_A));

  GamepadDevice second;
  second.x_info.minimum = 0;
  second.x_info.maximum = 255;
  second.y_info.minimum = 0;
  second.y_info.maximum = 255;
  second.x_value = 127;
  second.y_value = 255;
  second.raw_buttons = (1u << (BTN_THUMB - BTN_TRIGGER)) |
                       (1u << (BTN_BASE2 - BTN_TRIGGER)) |
                       (1u << (BTN_PINKIE - BTN_TRIGGER));
  assert(gamepad_state(second) == (PAD_DOWN | PAD_B | PAD_START | PAD_R));

  keyboard_state = PAD_SELECT;
  gamepads[0].state = gamepad_state(first);
  gamepads[1].state = gamepad_state(second);
  publish_states();
  assert(GetJoypadInput(0) == (PAD_SELECT | PAD_LEFT | PAD_A));
  assert(GetJoypadInput(1) == (PAD_DOWN | PAD_B | PAD_START | PAD_R));
}

int main(void) {
  test_keyboard_mapping();
  test_published_thegamepad_mapping();
  test_digital_axes();
  test_two_independent_players();
  std::puts("joypad_input_test: OK");
  return 0;
}
