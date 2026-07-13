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
  const std::string chip8_rom = directory + "/fixture.ch8";
  const std::string deck_config = directory + "/fixture.sexp";
  const std::string manifest = directory + "/games.tsv";
  const std::string volume_state = directory + "/volume.state";
  const std::string keymap_state = directory + "/keymap.state";

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
  const std::string deck_config_text = "corrupted on purpose\n";
  expect(write_file(deck_config, deck_config_text.data(),
                    deck_config_text.size()),
         "write Deck game config fixture");

  const std::string row =
      "fixture\tFIXTURE GAME\tnes\t" + rom + "\t#87AFD7\n";
  expect(write_file(manifest, row.data(), row.size()), "write manifest fixture");

  std::vector<GameEntry> games;
  std::string error;
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
  expect(volume_label(0) == "VOL OFF" && volume_label(42) == "VOL 42%",
         "volume display distinguishes mute from an audible percentage");
  expect(volume_after_menu_target(MenuTargetVolumeToggle, 42, 42) == 0,
         "volume display mutes an audible level");
  expect(volume_after_menu_target(MenuTargetVolumeToggle, 0, 42) == 42,
         "volume display restores the last audible level");
  expect(volume_after_menu_target(MenuTargetVolumeUp, 0, 42) == 42,
         "volume plus restores the last audible level");
  expect(volume_after_menu_target(MenuTargetVolumeDown, 0, 42) == 0,
         "volume minus leaves mute enabled");
  expect(kGameTitleScale == 2,
         "all game titles use one compact fixed font scale");

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
      "#!/bin/sh\nprintf '%s' \"$INFONES_VOLUME_PERCENT\" > \"$1.volume\"\n";
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

    child = run_game(emulator, games[0], 0, NULL, &framebuffer);
    expect(child.started && child.error.empty(), "start muted child");
    expect(WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0,
           "muted child exits cleanly");
    expect(read_file(captured) == "0", "muted child receives zero volume");
    unlink(captured.c_str());
  }

  const std::string terminal = directory + "/capture-keymap.sh";
  const std::string terminal_capture = directory + "/terminal.keymap";
  const std::string terminal_helper =
      "#!/bin/sh\nprintf '%s' \"$RETRO_DECK_KEYMAP\" > "
      "\"$TERMINAL_CAPTURE\"\n";
  expect(write_file(terminal, terminal_helper.data(), terminal_helper.size()),
         "write terminal fixture");
  expect(chmod(terminal.c_str(), 0700) == 0,
         "make terminal fixture executable");
  setenv("TERMINAL_CAPTURE", terminal_capture.c_str(), 1);
  {
    Framebuffer framebuffer;
    const ChildResult child =
        run_terminal(terminal, "cz", NULL, &framebuffer);
    expect(child.started && child.error.empty(), "start terminal child");
    expect(WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0,
           "terminal child exits cleanly");
    expect(read_file(terminal_capture) == "cz",
           "terminal child inherits selected keymap");
  }
  unsetenv("TERMINAL_CAPTURE");

  Canvas canvas;
  MenuLayout menu_layout;
  std::vector<GameEntry> tab_games = games;
  if (!games.empty()) {
    GameEntry second_nes = games[0];
    second_nes.id = "fixture-two";
    second_nes.title = "FIXTURE TWO";
    second_nes.color = xterm_color(174);
    tab_games.push_back(second_nes);
  }
  const char *additional_systems[] = {"gb", "gbc", "chip8", "deck"};
  for (size_t index = 0; index < 4 && !games.empty(); ++index) {
    GameEntry entry = games[0];
    entry.id = additional_systems[index];
    entry.title = additional_systems[index];
    entry.system = additional_systems[index];
    tab_games.push_back(entry);
  }
  const GameEntry terminal_entry = built_in_terminal_entry(terminal);
  expect(terminal_entry.title == "TERMINAL" &&
             terminal_entry.system == "deck" &&
             terminal_entry.rom == terminal &&
             is_built_in_terminal(terminal_entry),
         "built-in terminal is a routed Deck entry");
  tab_games.push_back(terminal_entry);
  render_menu(tab_games, "nes", 42, "us", false, 0, std::string(),
              &canvas, &menu_layout);
  expect(canvas.size() == static_cast<size_t>(kLogicalWidth * kLogicalHeight),
         "menu renders a complete logical canvas");
  expect(canvas[0] == xterm_pixel(kColorBackground),
         "menu background uses the xterm palette");
  expect(canvas[static_cast<size_t>(12) * kLogicalWidth + 26] ==
             xterm_pixel(kColorBackground),
         "menu omits the Retro Deck title");
  expect(menu_layout.terminal_button.width == 0 &&
             menu_layout.keymap_button.width == 0 &&
             menu_layout.wifi_button.width == 0 &&
             menu_layout.volume_down_button.width == 0 &&
             menu_layout.volume_display.width == 0 &&
             menu_layout.volume_up_button.width == 0,
         "console selector hides operational controls");
  expect(menu_layout.systems.size() == 5,
         "selector exposes each populated console");
  expect(menu_layout.game_buttons.empty(),
         "initial selector does not expose game targets");
  expect(canvas[static_cast<size_t>(menu_layout.system_button.y) *
                        kLogicalWidth +
                    menu_layout.system_button.x] == xterm_pixel(202),
         "console selector uses xterm orange 202");
  expect(rect_contains_color(canvas, menu_layout.system_button,
                             xterm_pixel(kColorBackground)),
         "console selector uses black label text");
  expect(menu_layout.system_button.height == 200,
         "console selector uses a tall primary button");
  expect(canvas[static_cast<size_t>(menu_layout.system_up_button.y) *
                        kLogicalWidth +
                    menu_layout.system_up_button.x] ==
                 xterm_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(menu_layout.system_down_button.y) *
                        kLogicalWidth +
                    menu_layout.system_down_button.x] ==
                 xterm_pixel(kColorBackground),
         "console arrows do not have background rectangles");
  expect(canvas[static_cast<size_t>(menu_layout.system_up_button.y +
                                    menu_layout.system_up_button.height / 2 -
                                    24) *
                        kLogicalWidth +
                    menu_layout.system_up_button.x +
                        menu_layout.system_up_button.width / 2] ==
                 xterm_pixel(202) &&
             canvas[static_cast<size_t>(menu_layout.system_down_button.y +
                                        menu_layout.system_down_button.height /
                                            2 -
                                        24) *
                            kLogicalWidth +
                        menu_layout.system_down_button.x +
                            menu_layout.system_down_button.width / 2] ==
                 xterm_pixel(202),
         "console arrow glyphs use xterm orange 202");
  expect(menu_layout.system_up_button.x >
                 menu_layout.system_button.x + menu_layout.system_button.width &&
             menu_layout.system_up_button.y == menu_layout.system_button.y &&
             menu_layout.system_down_button.y +
                     menu_layout.system_down_button.height ==
                 menu_layout.system_button.y + menu_layout.system_button.height,
         "console arrows align with its right top and bottom edges");
  expect(target_at(menu_layout, menu_layout.system_up_button.x + 1,
                   menu_layout.system_up_button.y + 1) == MenuTargetSystemUp &&
             target_at(menu_layout, menu_layout.system_button.x + 1,
                       menu_layout.system_button.y + 1) ==
                 MenuTargetSystemOpen &&
             target_at(menu_layout, menu_layout.system_down_button.x + 1,
                       menu_layout.system_down_button.y + 1) ==
                 MenuTargetSystemDown,
         "console selector surfaces have independent touch targets");
  expect(adjacent_system(menu_layout.systems, "nes", 1) == "gb" &&
             adjacent_system(menu_layout.systems, "nes", -1) == "deck",
         "console arrows wrap through populated systems");

  render_menu(tab_games, "nes", 42, "us", true, 0, std::string(), &canvas,
              &menu_layout);
  expect(menu_layout.terminal_button.x == 447 &&
             menu_layout.volume_up_button.x +
                     menu_layout.volume_up_button.width ==
                 833,
         "game view centers the compact operational controls");
  expect(menu_layout.terminal_button.height == 52 &&
             menu_layout.keymap_button.width == 52 &&
             menu_layout.wifi_button.width == 52 &&
             menu_layout.volume_display.width == 110 &&
             menu_layout.keymap_button.x ==
                 menu_layout.terminal_button.x +
                     menu_layout.terminal_button.width,
         "game view uses smaller controls without gaps");
  expect(target_at(menu_layout, menu_layout.volume_down_button.x + 1,
                   menu_layout.volume_down_button.y + 1) ==
                 MenuTargetVolumeDown &&
             target_at(menu_layout, menu_layout.volume_display.x + 1,
                       menu_layout.volume_display.y + 1) ==
                 MenuTargetVolumeToggle &&
             target_at(menu_layout, menu_layout.volume_up_button.x + 1,
                       menu_layout.volume_up_button.y + 1) ==
                 MenuTargetVolumeUp &&
             target_at(menu_layout, menu_layout.keymap_button.x + 1,
                       menu_layout.keymap_button.y + 1) == MenuTargetKeymap &&
             target_at(menu_layout, menu_layout.wifi_button.x + 1,
                       menu_layout.wifi_button.y + 1) == MenuTargetWifi &&
             target_at(menu_layout, menu_layout.terminal_button.x + 1,
                       menu_layout.terminal_button.y + 1) ==
                 MenuTargetTerminal,
         "game view operational controls retain their touch targets");
  expect(canvas[static_cast<size_t>(menu_layout.terminal_button.y) *
                        kLogicalWidth +
                    menu_layout.terminal_button.x] ==
                 xterm_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(menu_layout.keymap_button.y) *
                        kLogicalWidth +
                    menu_layout.keymap_button.x] ==
                 xterm_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(menu_layout.wifi_button.y) *
                        kLogicalWidth +
                    menu_layout.wifi_button.x] ==
                 xterm_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(menu_layout.volume_down_button.y) *
                        kLogicalWidth +
                    menu_layout.volume_down_button.x] ==
                 xterm_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(menu_layout.volume_up_button.y) *
                        kLogicalWidth +
                    menu_layout.volume_up_button.x] ==
                 xterm_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(menu_layout.volume_display.y) *
                        kLogicalWidth +
                    menu_layout.volume_display.x] ==
                 xterm_pixel(kColorBackground) &&
             rect_contains_color(canvas, menu_layout.volume_display,
                                 xterm_pixel(kColorVolumeOn)),
         "utility controls merge into black while volume text shows state");
  expect(canvas[static_cast<size_t>(menu_layout.terminal_button.y + 10) *
                        kLogicalWidth +
                    menu_layout.terminal_button.x + 11] ==
             xterm_pixel(kColorText),
         "terminal icon restores its 46-pixel-wide screen");
  expect(canvas[static_cast<size_t>(menu_layout.game_previous_button.y) *
                        kLogicalWidth +
                    menu_layout.game_previous_button.x] ==
                 xterm_pixel(kColorBackground) &&
             canvas[static_cast<size_t>(menu_layout.game_next_button.y) *
                        kLogicalWidth +
                    menu_layout.game_next_button.x] ==
                 xterm_pixel(kColorBackground),
         "carousel arrows do not have background rectangles");
  expect(rect_contains_color(canvas, menu_layout.game_previous_button,
                             xterm_pixel(202)) &&
             rect_contains_color(canvas, menu_layout.game_next_button,
                                 xterm_pixel(202)),
         "carousel arrow glyphs use xterm orange 202");
  expect(rects_are_horizontal_mirrors(canvas,
                                      menu_layout.game_previous_button,
                                      menu_layout.game_next_button),
         "carousel arrows are exact horizontal mirrors");
  expect(menu_layout.game_buttons.size() == 1 &&
             menu_layout.game_indices.size() == 2 &&
             menu_layout.shown_game_index == 0,
         "carousel exposes one game from the active console");
  expect(menu_layout.game_art.height == 268 &&
             menu_layout.game_art.y + menu_layout.game_art.height == 412 &&
             menu_layout.game_buttons[0].y +
                     menu_layout.game_buttons[0].height ==
                 446,
         "cover art uses the lower space while the title remains tappable");
  expect(menu_layout.game_previous_button.y * 2 +
                 menu_layout.game_previous_button.height ==
             menu_layout.game_art.y * 2 + menu_layout.game_art.height &&
             menu_layout.game_next_button.y ==
                 menu_layout.game_previous_button.y,
         "carousel arrows stay vertically centered on the taller cover");
  expect(canvas[static_cast<size_t>(menu_layout.game_art.y +
                                    menu_layout.game_art.height / 2) *
                        kLogicalWidth +
                    menu_layout.game_art.x + menu_layout.game_art.width / 2] ==
             cover_color.pixel(),
         "carousel centers the local cover inside the colored art tile");
  expect(menu_layout.game_position_indicators.size() == 2 &&
             menu_layout.game_position_indicators[0].y == 458 &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[0].y) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[0].x] ==
                 xterm_pixel(kColorFooter) &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[1].y) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[1].x] ==
                 xterm_pixel(kColorControlBorder) &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[0].y + 3) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[0].x + 3] ==
                 xterm_pixel(kColorBackground),
         "carousel position uses hollow bright and muted markers");
  expect(rect_contains_color(canvas, menu_layout.system_button,
                             xterm_pixel(kColorBackground)),
         "carousel console button uses black label text");
  expect(target_at(menu_layout, menu_layout.system_button.x + 1,
                   menu_layout.system_button.y + 1) == MenuTargetSystemBack &&
             target_at(menu_layout, menu_layout.game_previous_button.x + 1,
                       menu_layout.game_previous_button.y + 1) ==
                 MenuTargetGamePrevious &&
             target_at(menu_layout, menu_layout.game_next_button.x + 1,
                       menu_layout.game_next_button.y + 1) ==
                 MenuTargetGameNext,
         "carousel breadcrumb and arrows have touch targets");
  if (!menu_layout.game_buttons.empty()) {
    const Rect &game_button = menu_layout.game_buttons[0];
    expect(canvas[static_cast<size_t>(game_button.y) * kLogicalWidth +
                  game_button.x] == tab_games[0].color.pixel(),
           "carousel game uses its catalog color through the edge");
    expect(target_at(menu_layout, game_button.x + 1, game_button.y + 1) == 0,
           "carousel game launches its catalog entry");
  }

  render_menu(tab_games, "nes", 42, "us", true, 1, std::string(), &canvas,
              &menu_layout);
  expect(menu_layout.shown_game_index == 1 &&
             target_at(menu_layout, menu_layout.game_buttons[0].x + 1,
                       menu_layout.game_buttons[0].y + 1) == 1,
         "carousel position selects the next game mapping");
  expect(menu_layout.game_position_indicators.size() == 2 &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[0].y) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[0].x] ==
                 xterm_pixel(kColorControlBorder) &&
             canvas[static_cast<size_t>(
                        menu_layout.game_position_indicators[1].y) *
                            kLogicalWidth +
                    menu_layout.game_position_indicators[1].x] ==
                 xterm_pixel(kColorFooter),
         "carousel marker follows the selected game");

  render_menu(tab_games, "chip8", 0, "cz", true, 0, std::string(), &canvas,
              &menu_layout);
  expect(canvas[static_cast<size_t>(menu_layout.volume_display.y) *
                        kLogicalWidth +
                    menu_layout.volume_display.x] ==
                 xterm_pixel(kColorBackground) &&
             rect_contains_color(canvas, menu_layout.volume_display,
                                 xterm_pixel(kColorVolumeOff)),
         "muted volume display uses red text on black");
  expect(menu_layout.game_buttons.size() == 1 &&
             menu_layout.game_indices.size() == 1 &&
             menu_layout.shown_game_index == 4,
         "switching consoles changes the visible game mapping");
  if (!menu_layout.game_buttons.empty()) {
    expect(target_at(menu_layout, menu_layout.game_buttons[0].x + 1,
                     menu_layout.game_buttons[0].y + 1) == 4,
           "visible card launches its catalog game after filtering");
  }

  render_menu(tab_games, "deck", 42, "us", true, 1, std::string(), &canvas,
              &menu_layout);
  expect(menu_layout.game_indices.size() == 2 &&
             menu_layout.shown_game_index == 6 &&
             is_built_in_terminal(tab_games[menu_layout.shown_game_index]),
         "Deck carousel exposes the built-in terminal entry");

  WifiState wifi_state;
  WifiLayout wifi_layout;
  render_wifi(wifi_state, &canvas, &wifi_layout);
  expect(wifi_layout.keys.size() == 30,
         "alphabet keyboard exposes all letter and common SSID keys");
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
  render_wifi(wifi_state, &canvas, &wifi_layout);
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
      "--gb-emulator",  "/bin/false",       "--chip8-emulator",
      "/bin/echo",      "--deck-game",      "/bin/cat",
      "--manifest",     "/tmp/games",
      "--cover-directory", "/tmp/covers",
      "--volume-state", "/tmp/volume",      "--keymap-state",
      "/tmp/keymap",    "--terminal",       "/bin/false",
      "--wifi-helper",  "/bin/echo"};
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
             options.cover_directory == "/tmp/covers" &&
             options.volume_state == "/tmp/volume" &&
             options.keymap_state == "/tmp/keymap",
         "new executable options round-trip");
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
    routed.system = "deck";
    expect(emulator_for_game(options, routed) == "/bin/cat",
           "Deck entry selects native Deck game");
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
  unlink(volume_state.c_str());
  unlink(manifest.c_str());
  unlink(chip8_rom.c_str());
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
