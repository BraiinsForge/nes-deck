#ifndef RETRO_DECK_MENU_CATALOG_H
#define RETRO_DECK_MENU_CATALOG_H

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

extern const size_t kMaximumCatalogGames;

inline uint16_t rgb565(unsigned int red, unsigned int green,
                       unsigned int blue) {
  return static_cast<uint16_t>(((red & 0xf8) << 8) | ((green & 0xfc) << 3) |
                               (blue >> 3));
}

struct RgbColor {
  unsigned int red;
  unsigned int green;
  unsigned int blue;

  uint16_t pixel() const { return rgb565(red, green, blue); }
};

RgbColor xterm_color(unsigned int index);
bool is_xterm_color(const RgbColor &color);
inline uint16_t color_pixel(const RgbColor &color) { return color.pixel(); }
bool parse_color(const std::string &text, RgbColor *color);

struct CoverImage {
  int width;
  int height;
  std::vector<uint16_t> pixels;

  CoverImage() : width(0), height(0) {}
  bool available() const {
    return width > 0 && height > 0 &&
           pixels.size() == static_cast<size_t>(width * height);
  }
};

struct GameEntry {
  std::string id;
  std::string title;
  std::string system;
  std::string rom;
  RgbColor color;
  CoverImage cover;
};

bool valid_system(const std::string &system);
bool validate_rom(const std::string &system, const std::string &path,
                  std::string *error);
bool load_manifest(const std::string &path, std::vector<GameEntry> *games,
                   std::string *error);

GameEntry built_in_terminal_entry(const std::string &launcher);
bool is_built_in_terminal(const GameEntry &game);
GameEntry built_in_lua_entry(const std::string &launcher);
bool is_built_in_lua(const GameEntry &game);
GameEntry built_in_lisp_entry(const std::string &launcher);
bool is_built_in_lisp(const GameEntry &game);
GameEntry built_in_python_entry(const std::string &launcher);
bool is_built_in_python(const GameEntry &game);
GameEntry built_in_scheme_entry(const std::string &launcher);
bool is_built_in_scheme(const GameEntry &game);
GameEntry built_in_chiptune_entry(const std::string &directory);
bool is_built_in_chiptune(const GameEntry &game);
GameEntry built_in_reboot_entry(const std::string &executable);
bool is_built_in_reboot(const GameEntry &game);

std::string terminal_mode_for_game(const GameEntry &game);
std::string terminal_program_title(const std::string &mode);

#endif
