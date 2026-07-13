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
  const std::string manifest = directory + "/games.tsv";
  const std::string state = directory + "/sound.state";

  unsigned char ines[16] = {};
  std::memcpy(ines, "NES\x1a", 4);
  ines[4] = 1;
  ines[5] = 1;
  expect(write_file(rom, ines, sizeof(ines)), "write iNES fixture");

  const std::string row =
      "fixture\tFIXTURE GAME\t" + rom +
      "\tParser and persistent state test.\t#12ABEF\tMIT\n";
  expect(write_file(manifest, row.data(), row.size()), "write manifest fixture");

  std::vector<GameEntry> games;
  std::string error;
  expect(load_manifest(manifest, &games, &error), "load valid manifest");
  expect(games.size() == 1, "manifest contains one game");
  if (games.size() == 1) {
    expect(games[0].id == "fixture", "manifest id round-trips");
    expect(games[0].rom == rom, "manifest ROM path round-trips");
    expect(games[0].color.red == 0x12 && games[0].color.green == 0xab &&
               games[0].color.blue == 0xef,
           "manifest color parses");
  }

  bool sound_on = false;
  error.clear();
  expect(load_sound_state(state, &sound_on, &error),
         "missing sound state initializes");
  expect(sound_on, "new sound state defaults on");
  expect(read_file(state) == "on\n", "on state has canonical bytes");

  error.clear();
  expect(save_sound_state(state, false, &error), "save muted sound state");
  expect(read_file(state) == "off\n", "off state has canonical bytes");
  sound_on = true;
  expect(load_sound_state(state, &sound_on, &error), "reload muted sound state");
  expect(!sound_on, "muted sound state survives reload");

  const std::string invalid = "maybe\n";
  expect(write_file(state, invalid.data(), invalid.size()),
         "write invalid sound state fixture");
  expect(!load_sound_state(state, &sound_on, &error),
         "invalid sound state is rejected");

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
    ChildResult child =
        run_game(emulator, games[0], true, 63, NULL, &framebuffer);
    expect(child.started && child.error.empty(), "start sound-on child");
    expect(WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0,
           "sound-on child exits cleanly");
    expect(read_file(captured) == "63", "sound-on child inherits enabled volume");
    unlink(captured.c_str());

    child = run_game(emulator, games[0], false, 63, NULL, &framebuffer);
    expect(child.started && child.error.empty(), "start sound-off child");
    expect(WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0,
           "sound-off child exits cleanly");
    expect(read_file(captured) == "0", "sound-off child is muted");
    unlink(captured.c_str());
  }

  expect(geometry_test() == 0, "framebuffer transform geometry");

  unlink(emulator.c_str());
  unlink(state.c_str());
  unlink(manifest.c_str());
  unlink(rom.c_str());
  rmdir(directory.c_str());

  if (failures != 0)
    return 1;
  std::cout << "deck-menu-test: OK\n";
  return 0;
}
