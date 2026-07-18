#include <cassert>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

#include <cstdlib>
#include <sys/stat.h>
#include <unistd.h>

#include "../src/menu_catalog.h"

namespace {

void write_file(const std::string &path, const void *data, size_t size) {
  std::ofstream output(path.c_str(),
                       std::ios::out | std::ios::binary | std::ios::trunc);
  assert(output);
  output.write(static_cast<const char *>(data),
               static_cast<std::streamsize>(size));
  output.close();
  assert(output);
}

void write_text(const std::string &path, const std::string &contents) {
  write_file(path, contents.data(), contents.size());
}

void set_game_boy_checksum(unsigned char *rom) {
  unsigned char checksum = 0;
  for (size_t index = 0x134; index <= 0x14c; ++index)
    checksum = static_cast<unsigned char>(checksum - rom[index] - 1);
  rom[0x14d] = checksum;
}

} // namespace

int main() {
  char directory_template[] = "/tmp/menu-catalog-test-XXXXXX";
  char *directory_name = mkdtemp(directory_template);
  assert(directory_name);
  const std::string directory(directory_name);
  const std::string nes_path = directory + "/fixture.nes";
  const std::string gb_path = directory + "/fixture.gb";
  const std::string gbc_path = directory + "/fixture.gbc";
  const std::string zx_path = directory + "/fixture.tap";
  const std::string chip8_path = directory + "/fixture.ch8";
  const std::string manifest_path = directory + "/games.tsv";
  std::string error;

  RgbColor color = {0, 0, 0};
  assert(parse_color("#87AFD7", &color));
  assert(color.red == 0x87 && color.green == 0xaf && color.blue == 0xd7);
  assert(is_xterm_color(color));
  assert(!is_xterm_color(RgbColor{0x12, 0x34, 0x56}));
  assert(!parse_color("#12ZZ56", &color));
  assert(xterm_color(202).red == 255 && xterm_color(202).green == 95 &&
         xterm_color(202).blue == 0);
  assert(rgb565(255, 255, 255) == 0xffff);

  CoverImage cover;
  assert(!cover.available());
  cover.width = 1;
  cover.height = 1;
  cover.pixels.push_back(0xffff);
  assert(cover.available());

  unsigned char ines[16] = {};
  std::memcpy(ines, "NES\x1a", 4);
  ines[4] = 1;
  ines[5] = 1;
  write_file(nes_path, ines, sizeof(ines));

  static const unsigned char nintendo_logo[48] = {
      0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b, 0x03, 0x73, 0x00, 0x83,
      0x00, 0x0c, 0x00, 0x0d, 0x00, 0x08, 0x11, 0x1f, 0x88, 0x89, 0x00, 0x0e,
      0xdc, 0xcc, 0x6e, 0xe6, 0xdd, 0xdd, 0xd9, 0x99, 0xbb, 0xbb, 0x67, 0x63,
      0x6e, 0x0e, 0xec, 0xcc, 0xdd, 0xdc, 0x99, 0x9f, 0xbb, 0xb9, 0x33, 0x3e};
  unsigned char game_boy[0x150] = {};
  std::memcpy(game_boy + 0x104, nintendo_logo, sizeof(nintendo_logo));
  std::memcpy(game_boy + 0x134, "FIXTURE", 7);
  set_game_boy_checksum(game_boy);
  write_file(gb_path, game_boy, sizeof(game_boy));
  game_boy[0x143] = 0x80;
  set_game_boy_checksum(game_boy);
  write_file(gbc_path, game_boy, sizeof(game_boy));

  const unsigned char tap[] = {0x02, 0x00, 0xff, 0xff};
  write_file(zx_path, tap, sizeof(tap));
  const unsigned char chip8[] = {0x00, 0xe0, 0x12, 0x00};
  write_file(chip8_path, chip8, sizeof(chip8));

  assert(valid_system("nes") && valid_system("gb") && valid_system("gbc") &&
         valid_system("zx") && valid_system("chip8") && valid_system("deck") &&
         !valid_system("snes"));
  assert(validate_rom("nes", nes_path, &error));
  assert(validate_rom("gb", gb_path, &error));
  assert(validate_rom("gbc", gbc_path, &error));
  assert(!validate_rom("gbc", gb_path, &error));
  assert(validate_rom("zx", zx_path, &error));
  assert(validate_rom("chip8", chip8_path, &error));
  assert(validate_rom("deck", directory + "/optional-data", &error));
  assert(!validate_rom("nes", chip8_path, &error));
  assert(!validate_rom("nes", "relative.nes", &error));

  const unsigned char corrupt_tap[] = {0x02, 0x00, 0xff, 0x00};
  write_file(zx_path, corrupt_tap, sizeof(corrupt_tap));
  assert(!validate_rom("zx", zx_path, &error));
  write_file(zx_path, tap, sizeof(tap));

  const std::string valid_manifest = "id\ttitle\tsystem\trom\tcolor\n"
                                     "fixture\tFIXTURE GAME\tnes\t" +
                                     nes_path + "\t#87AFD7\n";
  write_text(manifest_path, valid_manifest);
  std::vector<GameEntry> games;
  assert(load_manifest(manifest_path, &games, &error));
  assert(games.size() == 1 && games[0].id == "fixture" &&
         games[0].title == "FIXTURE GAME" && games[0].system == "nes" &&
         games[0].rom == nes_path && games[0].color.red == 0x87);
  assert(!load_manifest("relative.tsv", &games, &error));

  write_text(manifest_path,
             "fixture\tFIXTURE\tnes\t" + nes_path + "\t#123456\n");
  assert(!load_manifest(manifest_path, &games, &error));
  assert(error.find("xterm-256") != std::string::npos);

  write_text(manifest_path, "fixture\tONE\tnes\t" + nes_path + "\t#87AFD7\n" +
                                "fixture\tTWO\tnes\t" + nes_path +
                                "\t#87AFD7\n");
  assert(!load_manifest(manifest_path, &games, &error));
  assert(error.find("duplicate id") != std::string::npos);

  write_text(manifest_path, "");
  assert(!load_manifest(manifest_path, &games, &error));
  assert(error.find("contains no games") != std::string::npos);

  std::ostringstream oversized_catalog;
  for (size_t index = 0; index <= kMaximumCatalogGames; ++index) {
    oversized_catalog << "deck-" << index << "\tDECK " << index
                      << "\tdeck\t/optional/" << index << "\t#87AFD7\n";
  }
  write_text(manifest_path, oversized_catalog.str());
  assert(!load_manifest(manifest_path, &games, &error));
  assert(error.find("more than 64 games") != std::string::npos);

  const GameEntry terminal = built_in_terminal_entry("/terminal");
  const GameEntry lua = built_in_lua_entry("/terminal");
  const GameEntry lisp = built_in_lisp_entry("/terminal");
  const GameEntry python = built_in_python_entry("/terminal");
  const GameEntry scheme = built_in_scheme_entry("/terminal");
  const GameEntry chiptunes = built_in_chiptune_entry("/music");
  const GameEntry reboot = built_in_reboot_entry("/sbin/reboot");
  assert(is_built_in_terminal(terminal));
  assert(is_built_in_lua(lua) && terminal_mode_for_game(lua) == "lua");
  assert(is_built_in_lisp(lisp) && terminal_mode_for_game(lisp) == "lisp");
  assert(is_built_in_python(python) &&
         terminal_mode_for_game(python) == "python");
  assert(is_built_in_scheme(scheme) &&
         terminal_mode_for_game(scheme) == "scheme");
  assert(is_built_in_chiptune(chiptunes));
  assert(is_built_in_reboot(reboot));
  assert(terminal_mode_for_game(terminal) == "shell");
  assert(terminal_program_title("scheme") == "SCHEME REPL");
  assert(terminal_program_title("shell") == "TERMINAL");

  assert(unlink(manifest_path.c_str()) == 0);
  assert(unlink(chip8_path.c_str()) == 0);
  assert(unlink(zx_path.c_str()) == 0);
  assert(unlink(gbc_path.c_str()) == 0);
  assert(unlink(gb_path.c_str()) == 0);
  assert(unlink(nes_path.c_str()) == 0);
  assert(rmdir(directory.c_str()) == 0);

  std::cout << "menu_catalog_test: OK\n";
  return 0;
}
