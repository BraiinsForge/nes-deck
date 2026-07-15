#include <iterator>

#define main deck_menu_cli_main
#include "../src/deck_menu.cpp"
#undef main

namespace {

int failures = 0;

void expect(bool condition, const char *message) {
  if (condition)
    return;
  std::cerr << "FAIL: " << message << '\n';
  ++failures;
}

bool write_file(const std::string &path, const void *data, size_t size) {
  const int fd = open(path.c_str(), O_WRONLY | O_CREAT | O_TRUNC, 0600);
  if (fd < 0)
    return false;
  const bool ok = write_all(fd, static_cast<const char *>(data), size);
  return close(fd) == 0 && ok;
}

std::string read_file(const std::string &path) {
  std::ifstream input(path.c_str(), std::ios::in | std::ios::binary);
  return std::string(std::istreambuf_iterator<char>(input),
                     std::istreambuf_iterator<char>());
}

bool rect_contains_color(const Canvas &canvas, const Rect &rect,
                         uint16_t color) {
  for (int y = rect.y; y < rect.y + rect.height; ++y) {
    for (int x = rect.x; x < rect.x + rect.width; ++x) {
      if (canvas[static_cast<size_t>(y) * kLogicalWidth + x] == color)
        return true;
    }
  }
  return false;
}

bool rects_are_horizontal_mirrors(const Canvas &canvas, const Rect &left,
                                  const Rect &right) {
  if (left.width != right.width || left.height != right.height)
    return false;
  for (int y = 0; y < left.height; ++y) {
    for (int x = 0; x < left.width; ++x) {
      const uint16_t left_pixel =
          canvas[static_cast<size_t>(left.y + y) * kLogicalWidth + left.x + x];
      const uint16_t right_pixel =
          canvas[static_cast<size_t>(right.y + y) * kLogicalWidth + right.x +
                 right.width - 1 - x];
      if (left_pixel != right_pixel)
        return false;
    }
  }
  return true;
}

} // namespace

int main() {
  char directory_template[] = "/tmp/deck-menu-test-XXXXXX";
  char *created = mkdtemp(directory_template);
  expect(created != NULL, "mkdtemp creates fixture directory");
  if (!created)
    return 1;

  const std::string directory(created);
  const std::string rom = directory + "/fixture.nes";
  const std::string gb_rom = directory + "/fixture.gb";
  const std::string gbc_rom = directory + "/fixture.gbc";
  const std::string zx_rom = directory + "/fixture.tap";
  const std::string chip8_rom = directory + "/fixture.ch8";
  const std::string deck_config = directory + "/fixture.sexp";
  const std::string manifest = directory + "/games.tsv";
  const std::string volume_state = directory + "/volume.state";
  const std::string brightness_file = directory + "/brightness";
  const std::string brightness_max_file = directory + "/max_brightness";
  const std::string brightness_state = directory + "/brightness.state";
  const std::string keymap_state = directory + "/keymap.state";
  const std::string palette_path = directory + "/palette.tsv";
  const std::string bad_palette_path = directory + "/bad-palette.tsv";
  std::string error;

  const std::string palette_fixture =
      "background\t#000000\ntext-dark\t#121212\nfield\t#121212\n"
      "surface\t#1C1C1C\ninactive-border\t#5F5F5F\n"
      "control-border\t#6C6C6C\nfooter\t#BCBCBC\n"
      "inactive-text\t#DADADA\ntext\t#EEEEEE\nwhite\t#FFFFFF\n"
      "title\t#FFFFAF\nvolume-off\t#AF8787\nvolume-on\t#87AF87\n"
      "selected\t#87AFAF\nwifi-active\t#5F87AF\n"
      "wifi-focus\t#87AFFF\nwifi-active-border\t#AFAFFF\n"
      "field-label\t#AFAFAF\naccent\t#123456\nactive\t#3A3A3A\n"
      "control-surface\t#303030\nmuted\t#654321\n";
  expect(write_file(palette_path, palette_fixture.data(),
                    palette_fixture.size()),
         "write complete dashboard palette fixture");
  error.clear();
  expect(load_dashboard_palette(palette_path, &error) &&
             kColorAccent.red == 0x12 && kColorAccent.green == 0x34 &&
             kColorAccent.blue == 0x56 && kColorMuted.red == 0x65 &&
             kColorMuted.green == 0x43 && kColorMuted.blue == 0x21,
         "complete full RGB dashboard palette loads");
  const std::string bad_palette = "background\t#12345G\n";
  expect(write_file(bad_palette_path, bad_palette.data(), bad_palette.size()),
         "write invalid dashboard palette fixture");
  error.clear();
  expect(!load_dashboard_palette(bad_palette_path, &error) &&
             kColorAccent.red == 0x12 && kColorAccent.green == 0x34 &&
             kColorAccent.blue == 0x56,
         "invalid palette is rejected without partially changing colors");
  reset_dashboard_palette();

  expect(menu_gamepad_key_to_button(BTN_THUMB2) == kMenuPadConfirm &&
             menu_gamepad_key_to_button(BTN_TOP) == kMenuPadConfirm &&
             menu_gamepad_key_to_button(BTN_THUMB) == kMenuPadBack &&
             menu_gamepad_key_to_button(BTN_TRIGGER) == kMenuPadBack &&
             menu_gamepad_key_to_button(BTN_TOP2) ==
                 kMenuPadSystemPrevious &&
             menu_gamepad_key_to_button(BTN_PINKIE) == kMenuPadSystemNext &&
             menu_gamepad_key_to_button(BTN_BASE) == kMenuPadSettings,
         "dashboard maps face and shoulder buttons to menu actions");
  expect(menu_gamepad_axis_to_button(0, 0, 255, kMenuPadLeft,
                                     kMenuPadRight) == kMenuPadLeft &&
             menu_gamepad_axis_to_button(127, 0, 255, kMenuPadLeft,
                                         kMenuPadRight) == 0 &&
             menu_gamepad_axis_to_button(255, 0, 255, kMenuPadLeft,
                                         kMenuPadRight) == kMenuPadRight,
         "dashboard applies a center dead zone to gamepad axes");
  expect(menu_keyboard_key_to_button(KEY_ENTER, false) == kMenuPadConfirm &&
             menu_keyboard_key_to_button(KEY_KPENTER, false) ==
                 kMenuPadConfirm &&
             menu_keyboard_key_to_button(KEY_ESC, false) == kMenuPadBack &&
             menu_keyboard_key_to_button(KEY_UP, false) == kMenuPadUp &&
             menu_keyboard_key_to_button(KEY_DOWN, false) == kMenuPadDown &&
             menu_keyboard_key_to_button(KEY_LEFT, false) == kMenuPadLeft &&
             menu_keyboard_key_to_button(KEY_RIGHT, false) == kMenuPadRight &&
             menu_keyboard_key_to_button(KEY_TAB, false) ==
                 kMenuPadSystemNext &&
             menu_keyboard_key_to_button(KEY_TAB, true) ==
                 kMenuPadSystemPrevious &&
             menu_keyboard_key_repeats(KEY_LEFT) &&
             menu_keyboard_key_repeats(KEY_DOWN) &&
             !menu_keyboard_key_repeats(KEY_ENTER) &&
             !menu_keyboard_key_repeats(KEY_TAB),
         "dashboard maps keyboard navigation and bounded repeat behavior");
  bool keyboard_left_shift = false;
  bool keyboard_right_shift = false;
  expect(menu_keyboard_event_to_button(KEY_TAB, 1, &keyboard_left_shift,
                                       &keyboard_right_shift) ==
                 kMenuPadSystemNext &&
             menu_keyboard_event_to_button(KEY_LEFTSHIFT, 1,
                                           &keyboard_left_shift,
                                           &keyboard_right_shift) == 0 &&
             menu_keyboard_event_to_button(KEY_TAB, 1, &keyboard_left_shift,
                                           &keyboard_right_shift) ==
                 kMenuPadSystemPrevious &&
             menu_keyboard_event_to_button(KEY_TAB, 2, &keyboard_left_shift,
                                           &keyboard_right_shift) == 0 &&
             menu_keyboard_event_to_button(KEY_RIGHT, 2,
                                           &keyboard_left_shift,
                                           &keyboard_right_shift) ==
                 kMenuPadRight &&
             menu_keyboard_event_to_button(KEY_LEFTSHIFT, 0,
                                           &keyboard_left_shift,
                                           &keyboard_right_shift) == 0 &&
             !keyboard_left_shift,
         "dashboard tracks Shift-Tab and accepts only arrow key repeats");
  const size_t keyboard_words =
      (KEY_MAX + sizeof(unsigned long) * CHAR_BIT) /
      (sizeof(unsigned long) * CHAR_BIT);
  std::vector<unsigned long> keyboard_keys(keyboard_words, 0);
  const auto set_keyboard_key = [&](unsigned int code) {
    const unsigned int bits_per_word = sizeof(unsigned long) * CHAR_BIT;
    keyboard_keys[code / bits_per_word] |= 1UL << (code % bits_per_word);
  };
  set_keyboard_key(KEY_ENTER);
  set_keyboard_key(KEY_ESC);
  set_keyboard_key(KEY_TAB);
  set_keyboard_key(KEY_UP);
  set_keyboard_key(KEY_DOWN);
  set_keyboard_key(KEY_LEFT);
  set_keyboard_key(KEY_RIGHT);
  expect(!menu_keyboard_capabilities(&keyboard_keys[0]),
         "dashboard rejects incomplete keyboard event devices");
  set_keyboard_key(KEY_LEFTSHIFT);
  expect(menu_keyboard_capabilities(&keyboard_keys[0]),
         "dashboard recognizes complete keyboard event devices");
  MenuGamepadDevice menu_pad;
  menu_pad.x_info.minimum = 0;
  menu_pad.x_info.maximum = 255;
  menu_pad.y_info.minimum = 0;
  menu_pad.y_info.maximum = 255;
  menu_pad.x_value = 255;
  menu_pad.y_value = 127;
  menu_pad.raw_buttons = 1u << (BTN_THUMB2 - BTN_TRIGGER);
  expect(menu_gamepad_state(menu_pad) ==
             (kMenuPadRight | kMenuPadConfirm),
         "dashboard combines D-pad and face-button state");
  expect(menu_gamepad_command(kMenuPadUp, false, false) ==
                 MenuGamepadCommandPrevious &&
             menu_gamepad_command(kMenuPadDown, false, false) ==
                 MenuGamepadCommandNext &&
             menu_gamepad_command(kMenuPadConfirm, false, false) ==
                 MenuGamepadCommandConfirm &&
             menu_gamepad_command(kMenuPadLeft, false, true) ==
                 MenuGamepadCommandPrevious &&
             menu_gamepad_command(kMenuPadRight, false, true) ==
                 MenuGamepadCommandNext &&
             menu_gamepad_command(kMenuPadBack, false, true) ==
                 MenuGamepadCommandBack &&
             menu_gamepad_command(kMenuPadBack, true, false) ==
                 MenuGamepadCommandBack &&
             menu_gamepad_command(kMenuPadSystemPrevious, false, false) ==
                 MenuGamepadCommandSystemPrevious &&
             menu_gamepad_command(kMenuPadSystemNext, false, false) ==
                 MenuGamepadCommandSystemNext &&
             menu_gamepad_command(kMenuPadSettings, false, false) ==
                 MenuGamepadCommandSettings &&
             menu_gamepad_command(kMenuPadSystemNext, false, true) ==
                 MenuGamepadCommandNone,
         "dashboard controller commands route games, systems, and settings");

  const std::vector<ChiptuneNote> volume_notes =
      menu_sound_notes(MenuSoundCueVolume);
  const std::vector<ChiptuneNote> previous_notes =
      menu_sound_notes(MenuSoundCuePrevious);
  const std::vector<ChiptuneNote> next_notes =
      menu_sound_notes(MenuSoundCueNext);
  const std::vector<ChiptuneNote> confirm_notes =
      menu_sound_notes(MenuSoundCueConfirm);
  const std::vector<ChiptuneNote> back_notes =
      menu_sound_notes(MenuSoundCueBack);
  expect(volume_notes.size() == 2 &&
             volume_notes[0].frequency < volume_notes[1].frequency &&
             chiptune_duration_ms(volume_notes) == 120 &&
             previous_notes.size() == 1 && next_notes.size() == 1 &&
             previous_notes[0].frequency < next_notes[0].frequency &&
             chiptune_duration_ms(previous_notes) == 35 &&
             chiptune_duration_ms(confirm_notes) == 55 &&
             confirm_notes[0].frequency < confirm_notes[1].frequency &&
             chiptune_duration_ms(back_notes) == 55 &&
             back_notes[0].frequency > back_notes[1].frequency,
         "dashboard sound cues stay short and directionally distinct");
  std::vector<int16_t> rendered_tone;
  std::string sound_error;
  expect(render_chiptune(volume_notes, 44100, 42, &rendered_tone,
                         &sound_error) &&
             rendered_tone.size() == 5292 &&
             !menu_input_quarantined(100, 100) &&
             menu_input_quarantined(101, 100),
         "dashboard renders bounded tones and exact input quarantine");
  expect(menu_sound_blocks_input(true, MenuInputTouch) &&
             menu_sound_blocks_input(true, MenuInputController) &&
             !menu_sound_blocks_input(true, MenuInputKeyboard) &&
             !menu_sound_blocks_input(false, MenuInputController),
         "menu sound quarantine keeps keyboard navigation responsive");

  MenuControllerInputGuard controller_guard;
  bool accepted_human_rate = true;
  for (int64_t edge = 0; edge < 12; ++edge)
    accepted_human_rate =
        accepted_human_rate && controller_guard.accept_edge(edge * 50);
  expect(accepted_human_rate && !controller_guard.suspended() &&
             !controller_guard.accept_edge(600) &&
             controller_guard.suspended() &&
             !controller_guard.recover_if_quiet(1599) &&
             controller_guard.recover_if_quiet(1600) &&
             !controller_guard.suspended() &&
             controller_guard.accept_edge(1601),
         "dashboard suspends impossible controller bursts until quiet");

  unsigned char ines[16] = {};
  std::memcpy(ines, "NES\x1a", 4);
  ines[4] = 1;
  ines[5] = 1;
  expect(write_file(rom, ines, sizeof(ines)), "write iNES fixture");

  static const unsigned char nintendo_logo[48] = {
      0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b,
      0x03, 0x73, 0x00, 0x83, 0x00, 0x0c, 0x00, 0x0d,
      0x00, 0x08, 0x11, 0x1f, 0x88, 0x89, 0x00, 0x0e,
      0xdc, 0xcc, 0x6e, 0xe6, 0xdd, 0xdd, 0xd9, 0x99,
      0xbb, 0xbb, 0x67, 0x63, 0x6e, 0x0e, 0xec, 0xcc,
      0xdd, 0xdc, 0x99, 0x9f, 0xbb, 0xb9, 0x33, 0x3e};
  unsigned char gameboy[0x150] = {};
  std::memcpy(gameboy + 0x104, nintendo_logo, sizeof(nintendo_logo));
  std::memcpy(gameboy + 0x134, "FIXTURE", 7);
  unsigned char checksum = 0;
  for (size_t index = 0x134; index <= 0x14c; ++index)
    checksum = static_cast<unsigned char>(checksum - gameboy[index] - 1);
  gameboy[0x14d] = checksum;
  expect(write_file(gb_rom, gameboy, sizeof(gameboy)),
         "write Game Boy fixture");
  gameboy[0x143] = 0x80;
  checksum = 0;
  for (size_t index = 0x134; index <= 0x14c; ++index)
    checksum = static_cast<unsigned char>(checksum - gameboy[index] - 1);
  gameboy[0x14d] = checksum;
  expect(write_file(gbc_rom, gameboy, sizeof(gameboy)),
         "write Game Boy Color fixture");
  const unsigned char chip8[] = {0x00, 0xe0, 0x12, 0x00};
  expect(write_file(chip8_rom, chip8, sizeof(chip8)),
         "write CHIP-8 fixture");
  const unsigned char tap[] = {0x02, 0x00, 0xff, 0xff};
  expect(write_file(zx_rom, tap, sizeof(tap)),
         "write checksummed ZX Spectrum TAP fixture");
  const std::string deck_config_text = "corrupted on purpose\n";
  expect(write_file(deck_config, deck_config_text.data(),
                    deck_config_text.size()),
         "write Deck game config fixture");

  const std::string row =
      "fixture\tFIXTURE GAME\tnes\t" + rom + "\t#87AFD7\n";
  expect(write_file(manifest, row.data(), row.size()), "write manifest fixture");

  std::vector<GameEntry> games;
  expect(load_manifest(manifest, &games, &error), "load valid manifest");
  expect(games.size() == 1, "manifest contains one game");
  if (games.size() == 1) {
    expect(games[0].id == "fixture", "manifest id round-trips");
    expect(games[0].system == "nes", "manifest system round-trips");
    expect(games[0].rom == rom, "manifest ROM path round-trips");
    expect(games[0].color.red == 0x87 && games[0].color.green == 0xaf &&
               games[0].color.blue == 0xd7,
           "manifest color parses");
  }

  const std::string cover_directory = directory + "/covers";
  expect(mkdir(cover_directory.c_str(), 0700) == 0,
         "create cover fixture directory");
  const RgbColor cover_color = xterm_color(202);
  std::string cover_data("P6\n2 2\n255\n");
  for (int pixel = 0; pixel < 4; ++pixel) {
    cover_data.push_back(static_cast<char>(cover_color.red));
    cover_data.push_back(static_cast<char>(cover_color.green));
    cover_data.push_back(static_cast<char>(cover_color.blue));
  }
  const std::string cover_path = cover_directory + "/fixture.ppm";
  expect(write_file(cover_path, cover_data.data(), cover_data.size()),
         "write xterm-quantized PPM cover fixture");
  expect(load_game_covers(cover_directory, &games) == 1 &&
             games[0].cover.available() && games[0].cover.width == 2 &&
             games[0].cover.height == 2 &&
             games[0].cover.pixels[0] == cover_color.pixel(),
         "load validated local cover into the catalog once");
  const std::string png_cover_path = cover_directory + "/fixture.png";
  png_image png_fixture;
  std::memset(&png_fixture, 0, sizeof(png_fixture));
  png_fixture.version = PNG_IMAGE_VERSION;
  png_fixture.width = 2;
  png_fixture.height = 2;
  png_fixture.format = PNG_FORMAT_RGBA;
  unsigned char png_pixels[16];
  for (size_t byte = 0; byte < sizeof(png_pixels); byte += 4) {
    png_pixels[byte] = cover_color.red;
    png_pixels[byte + 1] = cover_color.green;
    png_pixels[byte + 2] = cover_color.blue;
    png_pixels[byte + 3] = 255;
  }
  expect(png_image_write_to_file(&png_fixture, png_cover_path.c_str(), 0,
                                 png_pixels, 0, NULL) != 0,
         "write PNG cover fixture");
  games[0].cover = CoverImage();
  expect(load_game_covers(cover_directory, &games) == 1 &&
             games[0].cover.available() && games[0].cover.width == 2 &&
             games[0].cover.height == 2 &&
             games[0].cover.pixels[0] == cover_color.pixel(),
         "PNG cover takes priority and is quantized to xterm-256");

  const std::string non_palette_manifest =
      directory + "/non-palette-games.tsv";
  const std::string non_palette_row =
      "fixture\tFIXTURE GAME\tnes\t" + rom + "\t#12ABEF\n";
  expect(write_file(non_palette_manifest, non_palette_row.data(),
                    non_palette_row.size()),
         "write non-palette manifest fixture");
  std::vector<GameEntry> non_palette_games;
  error.clear();
  expect(!load_manifest(non_palette_manifest, &non_palette_games, &error) &&
             error.find("xterm-256") != std::string::npos,
         "manifest rejects colors outside xterm-256");
  unlink(non_palette_manifest.c_str());

  const std::string legacy_manifest = directory + "/legacy-games.tsv";
  const std::string legacy_row =
      "fixture\tFIXTURE GAME\t" + rom +
      "\tObsolete description.\t#12ABEF\tObsolete license\n";
  expect(write_file(legacy_manifest, legacy_row.data(), legacy_row.size()),
         "write legacy six-field manifest fixture");
  std::vector<GameEntry> rejected_games;
  error.clear();
  expect(!load_manifest(legacy_manifest, &rejected_games, &error),
         "legacy description and license fields are rejected");
  unlink(legacy_manifest.c_str());

  error.clear();
  expect(validate_rom("gb", gb_rom, &error), "valid GB header is accepted");
  expect(validate_rom("gbc", gbc_rom, &error),
         "valid GBC header is accepted");
  expect(!validate_rom("gbc", gb_rom, &error),
         "monochrome-only ROM is rejected as GBC");
  expect(validate_rom("chip8", chip8_rom, &error),
         "bounded CHIP-8 ROM is accepted");
  expect(validate_rom("zx", zx_rom, &error),
         "checksummed ZX Spectrum TAP is accepted");
  const unsigned char bad_tap[] = {0x02, 0x00, 0xff, 0x00};
  expect(write_file(zx_rom, bad_tap, sizeof(bad_tap)),
         "write corrupt ZX Spectrum TAP fixture");
  expect(!validate_rom("zx", zx_rom, &error),
         "ZX Spectrum TAP with a bad checksum is rejected");
  expect(validate_rom("deck", deck_config, &error),
         "Deck config remains launchable when its contents are corrupt");
  expect(validate_rom("deck", directory + "/missing.sexp", &error),
         "missing Deck config does not prevent launcher boot");
  expect(!validate_rom("nes", chip8_rom, &error),
         "CHIP-8 bytes are rejected as NES");

  unsigned int volume = 0;
  error.clear();
  expect(load_volume_state(volume_state, 42, &volume, &error),
         "missing volume state initializes");
  expect(volume == 42, "new volume state uses inherited default");
  expect(read_file(volume_state) == "42\n",
         "volume state has canonical bytes");

  error.clear();
  expect(save_volume_state(volume_state, 0, &error), "save muted volume state");
  expect(read_file(volume_state) == "0\n", "mute has canonical bytes");
  volume = 42;
  expect(load_volume_state(volume_state, 42, &volume, &error),
         "reload muted volume state");
  expect(volume == 0, "muted volume state survives reload");

  const std::string legacy_on = "on\n";
  expect(write_file(volume_state, legacy_on.data(), legacy_on.size()),
         "write legacy enabled sound state");
  expect(load_volume_state(volume_state, 37, &volume, &error) && volume == 37,
         "legacy enabled sound state migrates to the default volume");
  expect(read_file(volume_state) == "37\n",
         "legacy enabled sound state is rewritten canonically");

  const std::string legacy_off = "off\n";
  expect(write_file(volume_state, legacy_off.data(), legacy_off.size()),
         "write legacy muted sound state");
  expect(load_volume_state(volume_state, 37, &volume, &error) && volume == 0,
         "legacy muted sound state migrates to zero");
  expect(read_file(volume_state) == "0\n",
         "legacy muted sound state is rewritten canonically");

  const std::string invalid_volume = "042\n";
  expect(write_file(volume_state, invalid_volume.data(), invalid_volume.size()),
         "write invalid volume state fixture");
  expect(!load_volume_state(volume_state, 42, &volume, &error),
         "non-canonical volume state is rejected");
  expect(!save_volume_state(volume_state, 101, &error),
         "out-of-range volume cannot be saved");
  expect(volume_after_menu_target(SettingsTargetVolumeUp, 0, 42) == 42,
         "volume plus restores the last audible level");
  expect(volume_after_menu_target(SettingsTargetVolumeUp, 98, 42) == 100,
         "volume plus clamps at full volume");
  expect(volume_after_menu_target(SettingsTargetVolumeDown, 5, 42) == 0 &&
             volume_after_menu_target(SettingsTargetVolumeDown, 0, 42) == 0,
         "volume minus leaves mute enabled");
  const std::string brightness_bytes = "12\n";
  const std::string brightness_max_bytes = "20\n";
  expect(write_file(brightness_file, brightness_bytes.data(),
                    brightness_bytes.size()) &&
             write_file(brightness_max_file, brightness_max_bytes.data(),
                        brightness_max_bytes.size()),
         "write backlight fixtures");
  unsigned int maximum_brightness = 0;
  unsigned int brightness = 0;
  error.clear();
  expect(load_brightness(brightness_file, brightness_max_file,
                         brightness_state, &maximum_brightness, &brightness,
                         &error) &&
             maximum_brightness == 20 && brightness == 60 &&
             read_file(brightness_file) == "12\n" &&
             read_file(brightness_state) == "60\n",
         "missing brightness state adopts and persists the current backlight");
  expect(brightness_raw_value(10, 20) == 2 &&
             brightness_raw_value(60, 20) == 12 &&
             brightness_raw_value(100, 20) == 20,
         "brightness percentages map to the Deck backlight range");
  error.clear();
  expect(set_brightness_percent(brightness_file, brightness_state, 20, 70,
                                &error) &&
             read_file(brightness_file) == "14\n" &&
             read_file(brightness_state) == "70\n",
         "brightness changes update the backlight and persistent state");
  expect(brightness_after_settings_target(SettingsTargetBrightnessDown, 10) ==
                 10 &&
             brightness_after_settings_target(SettingsTargetBrightnessUp,
                                              100) == 100 &&
             brightness_after_settings_target(SettingsTargetBrightnessDown,
                                              60) == 50,
         "brightness controls clamp to the safe ten-percent range");
  const std::string invalid_brightness = "05\n";
  expect(write_file(brightness_state, invalid_brightness.data(),
                    invalid_brightness.size()) &&
             !load_brightness(brightness_file, brightness_max_file,
                              brightness_state, &maximum_brightness,
                              &brightness, &error),
         "non-canonical brightness state is rejected");
  expect(kGameTitleScale == 2,
         "all game titles use one compact fixed font scale");
  expect(kMaximumCoverWidth == 600 && kMaximumCoverHeight == 378,
         "cover decoding stays within the bounded art budget");
  expect(kMaxGames == 64,
         "catalog capacity leaves room for additional filed ROMs");

  std::string keymap;
  expect(load_keymap_state(keymap_state, &keymap, &error),
         "missing keymap state initializes");
  expect(keymap == "us" && read_file(keymap_state) == "us\n",
         "new terminal keymap defaults to US");
  expect(save_keymap_state(keymap_state, "cz", &error),
         "Czech terminal keymap saves");
  expect(load_keymap_state(keymap_state, &keymap, &error) && keymap == "cz",
         "Czech terminal keymap survives reload");
  const std::string invalid_keymap = "de\n";
  expect(write_file(keymap_state, invalid_keymap.data(), invalid_keymap.size()),
         "write invalid keymap fixture");
  expect(!load_keymap_state(keymap_state, &keymap, &error),
         "unsupported terminal keymap is rejected");

  unsigned int enabled_volume = 0;
  unsetenv("INFONES_VOLUME_PERCENT");
  error.clear();
  expect(inherited_volume(&enabled_volume, &error) && enabled_volume == 42,
         "missing enabled volume defaults to 42");
  setenv("INFONES_VOLUME_PERCENT", "63", 1);
  expect(inherited_volume(&enabled_volume, &error) && enabled_volume == 63,
         "enabled volume is inherited");
  setenv("INFONES_VOLUME_PERCENT", "101", 1);
  expect(!inherited_volume(&enabled_volume, &error),
         "out-of-range enabled volume is rejected");
  setenv("INFONES_VOLUME_PERCENT", "loud", 1);
  expect(!inherited_volume(&enabled_volume, &error),
         "non-numeric enabled volume is rejected");
  unsetenv("INFONES_VOLUME_PERCENT");

  const std::string emulator = directory + "/capture-volume.sh";
  const std::string captured = rom + ".volume";
  const std::string helper =
      "#!/bin/sh\n"
      "printf '%s' \"$INFONES_VOLUME_PERCENT\" > \"$1.volume\"\n"
      "printf '%s' \"${RETRO_DECK_VOLUME_STATE:-}\" > \"$1.volume-state\"\n";
  expect(write_file(emulator, helper.data(), helper.size()),
         "write emulator fixture");
  expect(chmod(emulator.c_str(), 0700) == 0, "make emulator fixture executable");
  if (!games.empty()) {
    Framebuffer framebuffer;
    ChildResult child = run_game(emulator, games[0], 63, NULL, &framebuffer);
    expect(child.started && child.error.empty(), "start volume child");
    expect(WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0,
           "volume child exits cleanly");
    expect(read_file(captured) == "63", "child inherits selected volume");
    unlink(captured.c_str());
    expect(read_file(rom + ".volume-state").empty(),
           "ordinary child receives no writable volume path");
    unlink((rom + ".volume-state").c_str());

    child = run_game(emulator, games[0], 0, NULL, &framebuffer);
    expect(child.started && child.error.empty(), "start muted child");
    expect(WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0,
           "muted child exits cleanly");
    expect(read_file(captured) == "0", "muted child receives zero volume");
    unlink(captured.c_str());
    unlink((rom + ".volume-state").c_str());

    child = run_game(emulator, games[0], 42, NULL, &framebuffer, volume_state);
    expect(child.started && WIFEXITED(child.status) &&
               WEXITSTATUS(child.status) == 0,
           "start volume-aware child");
    expect(read_file(rom + ".volume-state") == volume_state,
           "volume-aware child receives the exact persistent state path");
    unlink(captured.c_str());
    unlink((rom + ".volume-state").c_str());
  }

  const std::string terminal = directory + "/capture-keymap.sh";
  const std::string terminal_capture = directory + "/terminal.keymap";
  const std::string terminal_helper =
      "#!/bin/sh\nprintf '%s:%s' \"$RETRO_DECK_KEYMAP\" \"$1\" > "
      "\"$TERMINAL_CAPTURE\"\n";
  expect(write_file(terminal, terminal_helper.data(), terminal_helper.size()),
         "write terminal fixture");
  expect(chmod(terminal.c_str(), 0700) == 0,
         "make terminal fixture executable");
  setenv("TERMINAL_CAPTURE", terminal_capture.c_str(), 1);
  {
    Framebuffer framebuffer;
    const ChildResult child =
        run_terminal(terminal, "cz", "lisp", NULL, &framebuffer);
    expect(child.started && child.error.empty(), "start terminal child");
    expect(WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0,
           "terminal child exits cleanly");
    expect(read_file(terminal_capture) == "cz:lisp",
           "terminal child inherits the keymap and exact REPL mode");
  }
  unsetenv("TERMINAL_CAPTURE");
  {
    Framebuffer framebuffer;
    const ChildResult child = run_reboot("/bin/true", NULL, &framebuffer);
    expect(child.started && child.error.empty(), "start reboot command child");
    expect(WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0,
           "reboot command child exits cleanly");
  }
  expect(reboot_confirmation_active(5000, 4999) &&
             !reboot_confirmation_active(5000, 5000) &&
             !reboot_confirmation_active(0, 0),
         "reboot confirmation is bounded by its deadline");

  Canvas canvas;
  MenuLayout menu_layout;
  std::vector<GameEntry> tab_games = games;
  if (!games.empty()) {
    GameEntry second_nes = games[0];
    second_nes.id = "fixture-two";
    second_nes.title = "FIXTURE TWO";
    second_nes.color = xterm_color(174);
    second_nes.cover = CoverImage();
    tab_games.push_back(second_nes);
  }
  const char *additional_systems[] = {"gb", "gbc", "zx", "chip8", "deck"};
  for (size_t index = 0; index < 5 && !games.empty(); ++index) {
    GameEntry entry = games[0];
    entry.id = additional_systems[index];
    entry.title = additional_systems[index];
    entry.system = additional_systems[index];
    if (entry.system == "deck") {
      entry.id = "ten-seconds";
      entry.title = "10 SECONDS";
      entry.color = xterm_color(216);
      entry.cover = CoverImage();
    }
    tab_games.push_back(entry);
  }
  const GameEntry terminal_entry = built_in_terminal_entry(terminal);
  expect(terminal_entry.title == "TERMINAL" &&
             terminal_entry.system == "deck" &&
             terminal_entry.rom == terminal &&
             is_built_in_terminal(terminal_entry),
         "built-in terminal is a routed Deck entry");
  const GameEntry lua_entry = built_in_lua_entry(terminal);
  expect(lua_entry.title == "LUA REPL" && lua_entry.system == "deck" &&
             lua_entry.rom == terminal && is_built_in_lua(lua_entry) &&
             terminal_mode_for_game(lua_entry) == "lua",
         "built-in Lua REPL is a routed Deck entry");
  tab_games.push_back(lua_entry);
  const GameEntry lisp_entry = built_in_lisp_entry(terminal);
  expect(lisp_entry.title == "LISP REPL" && lisp_entry.system == "deck" &&
             lisp_entry.rom == terminal && is_built_in_lisp(lisp_entry) &&
             terminal_mode_for_game(lisp_entry) == "lisp",
         "built-in Lisp REPL is a routed Deck entry");
  tab_games.push_back(lisp_entry);
  const GameEntry python_entry = built_in_python_entry(terminal);
  expect(python_entry.title == "PYTHON REPL" &&
             python_entry.system == "deck" && python_entry.rom == terminal &&
             is_built_in_python(python_entry) &&
             terminal_mode_for_game(python_entry) == "python",
         "built-in Python REPL is a routed Deck entry");
  tab_games.push_back(python_entry);
  const GameEntry scheme_entry = built_in_scheme_entry(terminal);
  expect(scheme_entry.title == "SCHEME REPL" &&
             scheme_entry.system == "deck" && scheme_entry.rom == terminal &&
             is_built_in_scheme(scheme_entry) &&
             terminal_mode_for_game(scheme_entry) == "scheme",
         "built-in Scheme REPL is a routed Deck entry");
  tab_games.push_back(scheme_entry);
  const GameEntry chiptune_entry =
      built_in_chiptune_entry("/mnt/data/chiptunes");
  expect(chiptune_entry.title == "CHIPTUNES" &&
             chiptune_entry.system == "deck" &&
             chiptune_entry.rom == "/mnt/data/chiptunes" &&
             is_built_in_chiptune(chiptune_entry),
         "built-in chiptune player owns its persistent music directory");
  tab_games.push_back(chiptune_entry);
  expect(terminal_mode_for_game(terminal_entry) == "shell" &&
             terminal_program_title("shell") == "TERMINAL" &&
             terminal_program_title("lua") == "LUA REPL" &&
             terminal_program_title("lisp") == "LISP REPL" &&
             terminal_program_title("python") == "PYTHON REPL" &&
             terminal_program_title("scheme") == "SCHEME REPL",
         "terminal program modes have stable launcher labels");
  tab_games.push_back(terminal_entry);
  const GameEntry reboot_entry = built_in_reboot_entry("/bin/true");
  expect(reboot_entry.title == "REBOOT" && reboot_entry.system == "deck" &&
             reboot_entry.rom == "/bin/true" &&
             reboot_entry.color.red == 215 && reboot_entry.color.green == 95 &&
             reboot_entry.color.blue == 95 && is_built_in_reboot(reboot_entry),
         "built-in reboot is a red routed Deck entry");
  tab_games.push_back(reboot_entry);
  if (!games.empty()) {
    GameEntry third_nes = games[0];
    third_nes.id = "fixture-three";
    third_nes.title = "A VERY LONG FIXTURE GAME TITLE";
    third_nes.cover = CoverImage();
    tab_games.push_back(third_nes);
    GameEntry fourth_nes = third_nes;
    fourth_nes.id = "fixture-four";
    fourth_nes.title = "FIXTURE FOUR";
    fourth_nes.color = xterm_color(81);
    tab_games.push_back(fourth_nes);
  }

  render_menu(tab_games, "nes", 0, std::string(), &canvas, &menu_layout);
  expect(canvas.size() == static_cast<size_t>(kLogicalWidth * kLogicalHeight),
         "menu renders a complete logical canvas");
  expect(canvas[0] == color_pixel(kColorBackground),
         "menu background stays black");
  expect(menu_layout.systems.size() == 6 &&
             menu_layout.system_buttons.size() == 6,
         "main screen exposes every populated console as a tab");
  expect(adjacent_system(menu_layout.systems, "nes", 1) == "gb" &&
             adjacent_system(menu_layout.systems, "nes", -1) == "deck",
         "shoulder navigation wraps through populated systems");
  expect(system_label("gbc") == "GBC",
         "Game Boy Color uses the compact GBC tab label");
  const Rect &active_tab = menu_layout.system_buttons[0];
  const Rect &inactive_tab = menu_layout.system_buttons[1];
  expect(canvas[static_cast<size_t>(active_tab.y) * kLogicalWidth +
                    active_tab.x] == color_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(active_tab.y) * kLogicalWidth +
                    active_tab.x + kPixelStroke] ==
                 color_pixel(kColorAccent) &&
             canvas[static_cast<size_t>(active_tab.y + 12) * kLogicalWidth +
                    active_tab.x + 6] == color_pixel(kColorActive) &&
             canvas[static_cast<size_t>(inactive_tab.y + 12) * kLogicalWidth +
                    inactive_tab.x + 6] == color_pixel(kColorBackground),
         "tabs use cut orange borders and the translucent active fill");
  expect(target_at(menu_layout, active_tab.x + active_tab.width / 2,
                   active_tab.y + active_tab.height / 2) ==
                 MenuTargetSystemBase &&
             target_at(menu_layout,
                       menu_layout.settings_button.x +
                           menu_layout.settings_button.width / 2,
                       menu_layout.settings_button.y +
                           menu_layout.settings_button.height / 2) ==
                 MenuTargetSettings,
         "console tabs and the bottom-right settings control expose touch targets");
  expect(menu_layout.settings_button.x == 1212 &&
             menu_layout.settings_button.y == 412 &&
             rect_contains_color(canvas, menu_layout.settings_button,
                                 color_pixel(kColorFooter)),
         "dim retro cog sits at the bottom-right inset");
  expect(menu_layout.game_buttons.size() == 3 &&
             menu_layout.game_indices.size() == 4 &&
             menu_layout.visible_game_indices.size() == 3 &&
             menu_layout.visible_game_indices[0] == 0 &&
             menu_layout.visible_game_indices[1] == 1 &&
             menu_layout.visible_game_indices[2] == 14 &&
             menu_layout.shown_game_index == 0,
         "carousel shows at most three games while preserving the catalog");
  const Rect &selected_card = menu_layout.game_buttons[0];
  const Rect &inactive_card = menu_layout.game_buttons[1];
  expect(canvas[static_cast<size_t>(selected_card.y) * kLogicalWidth +
                    selected_card.x] == color_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(selected_card.y) * kLogicalWidth +
                    selected_card.x + kPixelStroke] ==
                 color_pixel(kColorAccent) &&
             canvas[static_cast<size_t>(selected_card.y + 12) * kLogicalWidth +
                    selected_card.x + 6] == color_pixel(kColorActive) &&
             canvas[static_cast<size_t>(inactive_card.y + 12) * kLogicalWidth +
                    inactive_card.x + 6] == color_pixel(kColorBackground),
         "selected game card reuses the active tab fill");
  expect(canvas[static_cast<size_t>(selected_card.y + 8) * kLogicalWidth +
                    selected_card.x + 8] == cover_color.pixel(),
         "game covers are cropped into the square art area");
  expect(target_at(menu_layout, selected_card.x + selected_card.width / 2,
                   selected_card.y + selected_card.height / 2) == 0 &&
             target_at(menu_layout, inactive_card.x + inactive_card.width / 2,
                       inactive_card.y + inactive_card.height / 2) == 1,
         "each visible game card launches its own catalog entry");
  expect(canvas[static_cast<size_t>(menu_layout.game_previous_button.y) *
                        kLogicalWidth +
                    menu_layout.game_previous_button.x] ==
                 color_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(menu_layout.game_next_button.y) *
                        kLogicalWidth +
                    menu_layout.game_next_button.x] ==
                 color_pixel(kColorBackground),
         "carousel arrows do not have background rectangles");
  expect(rect_contains_color(canvas, menu_layout.game_previous_button,
                             color_pixel(kColorFooter)) &&
             rect_contains_color(canvas, menu_layout.game_next_button,
                                 color_pixel(kColorFooter)),
         "carousel arrow glyphs use dim outlines");
  expect(rects_are_horizontal_mirrors(canvas,
                                      menu_layout.game_previous_button,
                                      menu_layout.game_next_button),
         "carousel arrows are exact horizontal mirrors");
  expect(menu_layout.game_position_indicators.size() == 4 &&
             menu_layout.game_position_indicators[0].y == 438 &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[0].y) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[0].x] ==
                 color_pixel(kColorFooter) &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[1].y) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[1].x] ==
                 color_pixel(kColorControlBorder) &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[0].y + 3) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[0].x + 3] ==
                 color_pixel(kColorBackground),
         "indicator row keeps one hollow marker per game");
  expect(target_at(menu_layout, menu_layout.game_previous_button.x + 1,
                       menu_layout.game_previous_button.y + 1) ==
                 MenuTargetGamePrevious &&
             target_at(menu_layout, menu_layout.game_next_button.x + 1,
                       menu_layout.game_next_button.y + 1) ==
                 MenuTargetGameNext,
         "carousel arrows have independent touch targets");

  render_menu(tab_games, "nes", 2, std::string(), &canvas, &menu_layout);
  expect(menu_layout.shown_game_index == 14 &&
             menu_layout.visible_game_indices[0] == 1 &&
             menu_layout.visible_game_indices[1] == 14 &&
             menu_layout.visible_game_indices[2] == 15 &&
             canvas[static_cast<size_t>(menu_layout.game_buttons[1].y + 12) *
                        kLogicalWidth +
                    menu_layout.game_buttons[1].x + 6] ==
                 color_pixel(kColorActive),
         "carousel window follows and fills the centered selected game");
  expect(menu_layout.game_position_indicators.size() == 4 &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[0].y) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[0].x] ==
                 color_pixel(kColorControlBorder) &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[2].y) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[2].x] ==
                 color_pixel(kColorFooter),
         "carousel marker follows the selected game");

  render_menu(tab_games, "chip8", 0, std::string(), &canvas, &menu_layout);
  expect(menu_layout.game_buttons.size() == 1 &&
             menu_layout.game_indices.size() == 1 &&
             menu_layout.shown_game_index == 5 &&
             menu_layout.game_previous_button.width == 0 &&
             menu_layout.game_next_button.width == 0,
         "switching consoles changes the visible game mapping");
  if (!menu_layout.game_buttons.empty()) {
    expect(target_at(menu_layout,
                     menu_layout.game_buttons[0].x +
                         menu_layout.game_buttons[0].width / 2,
                     menu_layout.game_buttons[0].y +
                         menu_layout.game_buttons[0].height / 2) == 5,
           "visible card launches its catalog game after filtering");
  }

  Canvas crop_canvas(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                     color_pixel(kColorBackground));
  CoverImage wide_cover;
  wide_cover.width = 4;
  wide_cover.height = 2;
  wide_cover.pixels = {xterm_color(1).pixel(), xterm_color(2).pixel(),
                       xterm_color(3).pixel(), xterm_color(4).pixel(),
                       xterm_color(5).pixel(), xterm_color(6).pixel(),
                       xterm_color(7).pixel(), xterm_color(8).pixel()};
  draw_cover_square(&crop_canvas, Rect{10, 10, 2, 2}, wide_cover);
  expect(crop_canvas[static_cast<size_t>(10) * kLogicalWidth + 10] ==
                 xterm_color(2).pixel() &&
             crop_canvas[static_cast<size_t>(10) * kLogicalWidth + 11] ==
                 xterm_color(3).pixel() &&
             crop_canvas[static_cast<size_t>(11) * kLogicalWidth + 10] ==
                 xterm_color(6).pixel(),
         "square covers use a centered crop instead of distortion");

  render_menu(tab_games, "deck", 0, std::string(), &canvas, &menu_layout);
  expect(menu_layout.game_indices.size() == 8 &&
             menu_layout.shown_game_index == 6 &&
             tab_games[menu_layout.shown_game_index].id == "ten-seconds",
         "Deck carousel exposes the Ten Seconds app entry");
  expect(rect_contains_color(
             canvas,
             Rect{menu_layout.game_buttons[0].x + 8,
                  menu_layout.game_buttons[0].y + 8,
                  menu_layout.game_buttons[0].width - 16,
                  menu_layout.game_buttons[0].width - 16},
             tab_games[6].color.pixel()),
         "Ten Seconds keeps its distinct compact app logo");

  render_menu(tab_games, "deck", 7, std::string(), &canvas, &menu_layout);
  expect(menu_layout.game_indices.size() == 8 &&
             menu_layout.shown_game_index == 13 &&
             is_built_in_reboot(tab_games[menu_layout.shown_game_index]),
         "Deck carousel exposes the built-in reboot entry");
  expect(menu_layout.visible_game_indices[2] == 13 &&
             rect_contains_color(
                 canvas,
                 Rect{menu_layout.game_buttons[2].x + 8,
                      menu_layout.game_buttons[2].y + 8,
                      menu_layout.game_buttons[2].width - 16,
                      menu_layout.game_buttons[2].width - 16},
                 tab_games[13].color.pixel()),
         "Reboot keeps the broken-ring power-on icon");

  Canvas terminal_icon_canvas(
      static_cast<size_t>(kLogicalWidth * kLogicalHeight),
      color_pixel(kColorBackground));
  const Rect terminal_icon_bounds{100, 100, 112, 104};
  draw_terminal_icon(&terminal_icon_canvas, terminal_icon_bounds,
                     color_pixel(kColorText));
  int terminal_icon_top = kLogicalHeight;
  int terminal_icon_bottom = -1;
  for (int y = terminal_icon_bounds.y;
       y < terminal_icon_bounds.y + terminal_icon_bounds.height; ++y) {
    for (int x = terminal_icon_bounds.x;
         x < terminal_icon_bounds.x + terminal_icon_bounds.width; ++x) {
      if (terminal_icon_canvas[static_cast<size_t>(y) * kLogicalWidth + x] ==
          color_pixel(kColorText)) {
        terminal_icon_top = std::min(terminal_icon_top, y);
        terminal_icon_bottom = std::max(terminal_icon_bottom, y);
      }
    }
  }
  expect(terminal_icon_top + terminal_icon_bottom + 1 ==
                 terminal_icon_bounds.y * 2 + terminal_icon_bounds.height &&
             std::string(kTerminalLoginShell) == "/BIN/ASH",
         "terminal icon is vertically centered and names its login shell");

  SettingsLayout settings_layout;
  NetworkStatus network_status;
  network_status.ssid = "net1";
  network_status.wlan_ipv4 = "10.249.110.248";
  network_status.wireguard_ipv4 = "10.0.0.10";
  network_status.selector = "CONNECTED";
  render_settings(42, 60, "us", SettingsTargetVolumeDown, std::string(),
                  network_status, &canvas, &settings_layout);
  expect(settings_layout.close_button.x == 1212 &&
             settings_layout.close_button.y == 12 &&
             settings_target_at(settings_layout,
                                settings_layout.close_button.x + 20,
                                settings_layout.close_button.y + 20) ==
                 SettingsTargetClose &&
             settings_target_at(settings_layout,
                                settings_layout.wifi_button.x + 20,
                                settings_layout.wifi_button.y + 20) ==
                 SettingsTargetWifi,
         "settings cross stays at top right and WiFi remains alongside it");
  expect(settings_target_at(settings_layout,
                            settings_layout.volume_down_button.x + 20,
                            settings_layout.volume_down_button.y + 20) ==
                 SettingsTargetVolumeDown &&
             settings_target_at(settings_layout,
                                settings_layout.volume_up_button.x + 20,
                                settings_layout.volume_up_button.y + 20) ==
                 SettingsTargetVolumeUp &&
             settings_target_at(settings_layout,
                                settings_layout.brightness_down_button.x + 20,
                                settings_layout.brightness_down_button.y + 20) ==
                 SettingsTargetBrightnessDown &&
             settings_target_at(settings_layout,
                                settings_layout.brightness_up_button.x + 20,
                                settings_layout.brightness_up_button.y + 20) ==
                 SettingsTargetBrightnessUp &&
             settings_target_at(settings_layout,
                                settings_layout.terminal_button.x + 20,
                                settings_layout.terminal_button.y + 20) ==
                 SettingsTargetTerminal &&
             settings_target_at(settings_layout,
                                settings_layout.keymap_button.x + 20,
                                settings_layout.keymap_button.y + 20) ==
                 SettingsTargetKeymap,
         "settings exposes volume, brightness, terminal, and key controls");
  expect(canvas[static_cast<size_t>(settings_layout.volume_down_button.y + 12) *
                        kLogicalWidth +
                    settings_layout.volume_down_button.x + 6] ==
                 color_pixel(kColorActive) &&
             canvas[static_cast<size_t>(settings_layout.volume_up_button.y +
                                        12) *
                        kLogicalWidth +
                    settings_layout.volume_up_button.x + 6] ==
                 color_pixel(kColorControlSurface),
         "controller-selected setting uses the active fill");
  render_settings(0, 100, "cz", SettingsTargetWifi, std::string(),
                  network_status, &canvas, &settings_layout);
  expect(canvas[static_cast<size_t>(settings_layout.wifi_button.y + 12) *
                        kLogicalWidth +
                    settings_layout.wifi_button.x + 6] ==
                 color_pixel(kColorActive) &&
             rect_contains_color(canvas, settings_layout.keymap_button,
                                 color_pixel(kColorText)),
         "settings renders muted volume, full brightness, and Czech keys");

  WifiState wifi_state;
  WifiLayout wifi_layout;
  render_wifi(wifi_state, network_status, &canvas, &wifi_layout);
  expect(wifi_layout.keys.size() == 30,
         "alphabet keyboard exposes all letter and common SSID keys");
  expect(wifi_layout.keys[0].value == 'q' &&
             std::memcmp(glyph_rows('q'), glyph_rows('Q'), 7) != 0,
         "lowercase Wi-Fi keys use distinct lowercase bitmap glyphs");
  const Canvas lowercase_wifi = canvas;
  wifi_state.uppercase = true;
  render_wifi(wifi_state, network_status, &canvas, &wifi_layout);
  expect(wifi_layout.keys[0].value == 'Q' && canvas != lowercase_wifi,
         "uppercase mode visibly changes the keyboard and its case control");
  wifi_state.uppercase = false;
  render_wifi(wifi_state, network_status, &canvas, &wifi_layout);
  expect(apply_wifi_target(WifiTargetKeyBase, wifi_layout, &wifi_state),
         "keyboard character target applies");
  expect(wifi_state.ssid == "q", "keyboard enters into selected SSID field");
  expect(apply_wifi_target(WifiTargetPassphrase, wifi_layout, &wifi_state),
         "password field can be selected");
  expect(apply_wifi_target(WifiTargetSpace, wifi_layout, &wifi_state) &&
             wifi_state.passphrase == " ",
         "space action edits selected field");
  expect(apply_wifi_target(WifiTargetDelete, wifi_layout, &wifi_state) &&
             wifi_state.passphrase.empty(),
         "delete action edits selected field");
  expect(apply_wifi_target(WifiTargetMode, wifi_layout, &wifi_state) &&
             wifi_state.symbols,
         "symbol keyboard can be selected");
  render_wifi(wifi_state, network_status, &canvas, &wifi_layout);
  expect(wifi_layout.keys.size() == 42,
         "symbol keyboard exposes every printable punctuation key");

  const std::string wifi_helper = directory + "/capture-wifi.sh";
  const std::string wifi_capture = directory + "/wifi.input";
  const std::string wifi_script =
      "#!/bin/sh\n"
      "umask 077\n"
      "cat > \"$WIFI_CAPTURE\"\n";
  expect(write_file(wifi_helper, wifi_script.data(), wifi_script.size()),
         "write wifi helper fixture");
  expect(chmod(wifi_helper.c_str(), 0700) == 0,
         "make wifi helper fixture executable");
  setenv("WIFI_CAPTURE", wifi_capture.c_str(), 1);
  error.clear();
  expect(save_wifi_profile(wifi_helper, "test net", "secret!9", &error),
         "wifi credentials are delivered through helper stdin");
  expect(read_file(wifi_capture) == "test net\nsecret!9\n",
         "wifi helper receives exact credentials");
  expect(!save_wifi_profile(wifi_helper, "test net", "short", &error),
         "short wifi password is rejected before helper execution");
  unsetenv("WIFI_CAPTURE");

  Options options;
  const char *option_values[] = {
      "deck-menu",      "--nes-emulator",   "/bin/true",
      "--gb-emulator",  "/bin/false",       "--zx-emulator",
      "/bin/printf",    "--chip8-emulator", "/bin/echo",
      "--deck-game",    "/bin/cat",
      "--chiptune-player", "/bin/sleep", "--chiptune-directory",
      "/tmp/chiptunes",
      "--manifest",     "/tmp/games",
      "--palette",      "/tmp/palette",
      "--cover-directory", "/tmp/covers",
      "--volume-state", "/tmp/volume",      "--brightness",
      "/tmp/brightness", "--brightness-max", "/tmp/max-brightness",
      "--brightness-state", "/tmp/brightness-state", "--keymap-state",
      "/tmp/keymap",    "--terminal",       "/bin/false",
      "--wifi-helper",  "/bin/echo",        "--wifi-status",
      "/var/run/deck-wifi/status"};
  char *option_argv[sizeof(option_values) / sizeof(option_values[0])];
  for (size_t index = 0; index < sizeof(option_values) / sizeof(option_values[0]);
       ++index)
    option_argv[index] = const_cast<char *>(option_values[index]);
  error.clear();
  expect(parse_options(static_cast<int>(sizeof(option_argv) /
                                        sizeof(option_argv[0])),
                       option_argv, &options, &error),
         "terminal and wifi helper options parse");
  expect(options.terminal == "/bin/false" &&
             options.wifi_helper == "/bin/echo" &&
             options.palette == "/tmp/palette" &&
             options.wifi_status == "/var/run/deck-wifi/status" &&
             options.chiptune_player == "/bin/sleep" &&
             options.chiptune_directory == "/tmp/chiptunes" &&
             options.cover_directory == "/tmp/covers" &&
             options.volume_state == "/tmp/volume" &&
             options.brightness == "/tmp/brightness" &&
             options.brightness_max == "/tmp/max-brightness" &&
             options.brightness_state == "/tmp/brightness-state" &&
             options.keymap_state == "/tmp/keymap",
         "new executable options round-trip");
  Options validation_options;
  const char *validation_values[] = {"deck-menu", "--validate-manifest",
                                     manifest.c_str()};
  char *validation_argv[] = {
      const_cast<char *>(validation_values[0]),
      const_cast<char *>(validation_values[1]),
      const_cast<char *>(validation_values[2])};
  error.clear();
  expect(parse_options(3, validation_argv, &validation_options, &error) &&
             validation_options.validate_manifest == manifest,
         "standalone manifest validation option parses");
  if (!games.empty()) {
    expect(emulator_for_game(options, games[0]) == "/bin/true",
           "NES entry selects NES emulator");
    GameEntry routed = games[0];
    routed.system = "gbc";
    expect(emulator_for_game(options, routed) == "/bin/false",
           "GBC entry selects shared GB emulator");
    routed.system = "chip8";
    expect(emulator_for_game(options, routed) == "/bin/echo",
           "CHIP-8 entry selects CHIP-8 emulator");
    routed.system = "zx";
    expect(emulator_for_game(options, routed) == "/bin/printf",
           "ZX entry selects ZX Spectrum emulator");
    routed.system = "deck";
    expect(emulator_for_game(options, routed) == "/bin/cat",
           "Deck entry selects native Deck game");
    routed = built_in_chiptune_entry("/tmp/chiptunes");
    expect(emulator_for_game(options, routed) == "/bin/sleep",
           "chiptune entry selects the dedicated native player");
  }

  expect(geometry_test() == 0, "framebuffer transform geometry");

  unlink(wifi_capture.c_str());
  unlink(wifi_helper.c_str());
  unlink(terminal_capture.c_str());
  unlink(terminal.c_str());
  unlink(cover_path.c_str());
  unlink(png_cover_path.c_str());
  rmdir(cover_directory.c_str());
  unlink(emulator.c_str());
  unlink(keymap_state.c_str());
  unlink(brightness_state.c_str());
  unlink(brightness_max_file.c_str());
  unlink(brightness_file.c_str());
  unlink(volume_state.c_str());
  unlink(manifest.c_str());
  unlink(chip8_rom.c_str());
  unlink(zx_rom.c_str());
  unlink(deck_config.c_str());
  unlink(gbc_rom.c_str());
  unlink(gb_rom.c_str());
  unlink(rom.c_str());
  rmdir(directory.c_str());

  if (failures != 0)
    return 1;
  std::cout << "deck-menu-test: OK\n";
  return 0;
}
