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
      "fixture\tFIXTURE GAME\tnes\t" + rom + "\t#12ABEF\n";
  expect(write_file(manifest, row.data(), row.size()), "write manifest fixture");

  std::vector<GameEntry> games;
  std::string error;
  expect(load_manifest(manifest, &games, &error), "load valid manifest");
  expect(games.size() == 1, "manifest contains one game");
  if (games.size() == 1) {
    expect(games[0].id == "fixture", "manifest id round-trips");
    expect(games[0].system == "nes", "manifest system round-trips");
    expect(games[0].rom == rom, "manifest ROM path round-trips");
    expect(games[0].color.red == 0x12 && games[0].color.green == 0xab &&
               games[0].color.blue == 0xef,
           "manifest color parses");
  }

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
  const char *additional_systems[] = {"gb", "gbc", "chip8", "deck"};
  for (size_t index = 0; index < 4 && !games.empty(); ++index) {
    GameEntry entry = games[0];
    entry.id = additional_systems[index];
    entry.title = additional_systems[index];
    entry.system = additional_systems[index];
    tab_games.push_back(entry);
  }
  render_menu(tab_games, "nes", 42, "us", std::string(), &canvas,
              &menu_layout);
  expect(canvas.size() == static_cast<size_t>(kLogicalWidth * kLogicalHeight),
         "menu renders a complete logical canvas");
  expect(canvas[0] == rgb565(0, 0, 0), "menu background is solid black");
  expect(canvas[static_cast<size_t>(82) * kLogicalWidth] == rgb565(0, 0, 0),
         "menu has no colored header divider");
  expect(target_at(menu_layout, menu_layout.volume_down_button.x + 1,
                   menu_layout.volume_down_button.y + 1) == -2,
         "volume down action has its own target");
  expect(target_at(menu_layout, menu_layout.volume_up_button.x + 1,
                   menu_layout.volume_up_button.y + 1) == -5,
         "volume up action has its own target");
  expect(target_at(menu_layout, menu_layout.keymap_button.x + 1,
                   menu_layout.keymap_button.y + 1) == -6,
         "keymap action has its own target");
  expect(target_at(menu_layout, menu_layout.wifi_button.x + 1,
                   menu_layout.wifi_button.y + 1) == -3,
         "wifi action has its own target");
  expect(target_at(menu_layout, menu_layout.terminal_button.x + 1,
                   menu_layout.terminal_button.y + 1) == -4,
         "terminal action has its own target");
  expect(menu_layout.system_tabs.size() == 5,
         "menu exposes one tab for each populated system");
  if (menu_layout.system_tabs.size() == 5) {
    const Rect &selected_tab = menu_layout.system_tabs[0].bounds;
    const Rect &inactive_tab = menu_layout.system_tabs[1].bounds;
    expect(canvas[static_cast<size_t>(selected_tab.y) * kLogicalWidth +
                  selected_tab.x] == rgb565(216, 205, 164),
           "selected system tab stays flat through its edge");
    expect(canvas[static_cast<size_t>(inactive_tab.y) * kLogicalWidth +
                  inactive_tab.x] == rgb565(25, 25, 25),
           "inactive system tab stays flat through its edge");
  }
  expect(menu_layout.game_buttons.size() == 1 &&
             menu_layout.game_indices.size() == 1 &&
             menu_layout.game_indices[0] == 0,
         "active console filters the visible game cards");
  if (!menu_layout.game_buttons.empty()) {
    const Rect &game_button = menu_layout.game_buttons[0];
    expect(canvas[static_cast<size_t>(game_button.y) * kLogicalWidth +
                  game_button.x] == tab_games[0].color.pixel(),
           "game tiles use their catalog color through the edge");
  }
  if (menu_layout.system_tabs.size() == 5) {
    expect(target_at(menu_layout, menu_layout.system_tabs[4].bounds.x + 1,
                     menu_layout.system_tabs[4].bounds.y + 1) ==
               kSystemTargetBase - 4,
           "system tab has its own touch target");
  }
  render_menu(tab_games, "chip8", 0, "cz", std::string(), &canvas,
              &menu_layout);
  expect(menu_layout.game_buttons.size() == 1 &&
             menu_layout.game_indices.size() == 1 &&
             menu_layout.game_indices[0] == 3,
         "switching consoles changes the visible game mapping");
  if (!menu_layout.game_buttons.empty()) {
    expect(target_at(menu_layout, menu_layout.game_buttons[0].x + 1,
                     menu_layout.game_buttons[0].y + 1) == 3,
           "visible card launches its catalog game after filtering");
  }

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
