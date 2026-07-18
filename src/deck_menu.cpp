/*
 * deck_menu.cpp - small touch-first launcher for the Braiins Deck
 *
 * Runtime interface:
 *
 *   deck-menu --nes-emulator /absolute/path/to/nes-deck \
 *             --gb-emulator /absolute/path/to/gb-deck \
 *             --zx-emulator /absolute/path/to/zx-deck \
 *             --chip8-emulator /absolute/path/to/chip8-deck \
 *             --deck-game /absolute/path/to/ten-seconds-deck \
 *             --chiptune-player /absolute/path/to/chiptune-deck \
 *             --chiptune-directory /absolute/path/to/chiptunes \
 *             --manifest /absolute/path/to/games.tsv \
 *             --settings-icon-directory /absolute/path/to/settings-icons \
 *             --cover-directory /absolute/path/to/covers \
 *             --volume-state /absolute/path/to/volume.state \
 *             --brightness /sys/class/backlight/display-bl/brightness \
 *             --brightness-max /sys/class/backlight/display-bl/max_brightness \
 *             --brightness-state /absolute/path/to/brightness.state \
 *             --keymap-state /absolute/path/to/keymap.state \
 *             --terminal /absolute/path/to/terminal-launcher \
 *             --wifi-helper /absolute/path/to/profile-writer
 *
 * Manifest rows have exactly five tab-separated fields:
 *
 *   id<TAB>title<TAB>system<TAB>rom<TAB>#RRGGBB
 *
 * Blank lines and lines beginning with '#' are ignored.  An optional header
 * row is accepted.  Volume state is a canonical integer from 0 through 100;
 * terminal keymap state is exactly "us" or "cz".
 *
 * This program uses C++11/POSIX APIs with either the BMC Wayland compositor or
 * Linux fbdev/evdev. The direct framebuffer is a 600x1280 portrait RGB565
 * surface, so its fallback renderer uses this transform:
 *
 *   framebuffer column = logical y
 *   framebuffer row    = 1279 - logical x
 */

#include <algorithm>
#include <arpa/inet.h>
#include <cerrno>
#include <cctype>
#include <climits>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dirent.h>
#include <fcntl.h>
#include <ifaddrs.h>
#include <fstream>
#include <iostream>
#include <linux/fb.h>
#include <linux/input.h>
#include <linux/kd.h>
#include <linux/wireless.h>
#include <poll.h>
#include <png.h>
#include <limits>
#include <set>
#include <sstream>
#include <string>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>
#include <utility>
#include <vector>

#include "menu_sound.h"

#ifdef RETRO_DECK_WAYLAND
#include "deck_wayland.h"
#endif

namespace {

const int kLogicalWidth = 1280;
const int kLogicalHeight = 480;
const int kPhysicalWidth = 600;
const int kPhysicalHeight = 1280;
const int kMaxGames = 64;
const off_t kMaximumManifestBytes = 65536;
const off_t kMaximumPaletteBytes = 4096;
const int kMaximumCoverWidth = 600;
const int kMaximumCoverHeight = 378;
const off_t kMaximumCoverBytes =
    1024 + kMaximumCoverWidth * kMaximumCoverHeight * 3;
const off_t kMaximumPngCoverBytes = 4 * 1024 * 1024;
const png_uint_32 kMaximumPngDimension = 2048;
// Touch is not a gameplay input. A hold anywhere is unambiguous and avoids an
// invisible corner target that may be confused with the inset NES image.
const int kExitHoldWidth = kLogicalWidth;
const int kExitHoldHeight = kLogicalHeight;
const int64_t kExitHoldMs = 2000;
const int64_t kChildTermGraceMs = 4000;
const int64_t kRebootConfirmMs = 4000;
const unsigned int kVolumeStep = 5;
const unsigned int kBrightnessStep = 10;
const unsigned int kMinimumBrightness = 10;
const int kGameTitleScale = 2;
const int kPixelStroke = 4;
const char kRebootExecutable[] = "/sbin/reboot";
const char kRebootConfirmationText[] = "PRESS A OR TAP AGAIN TO REBOOT";
const char kTerminalLoginShell[] = "/BIN/ASH";

const unsigned int kMenuPadConfirm = 1u << 0;
const unsigned int kMenuPadBack = 1u << 1;
const unsigned int kMenuPadUp = 1u << 2;
const unsigned int kMenuPadDown = 1u << 3;
const unsigned int kMenuPadLeft = 1u << 4;
const unsigned int kMenuPadRight = 1u << 5;
const unsigned int kMenuPadSystemPrevious = 1u << 6;
const unsigned int kMenuPadSystemNext = 1u << 7;
const unsigned int kMenuPadSettings = 1u << 8;
const unsigned short kTheGamepadVendor = 0x1c59;
const unsigned short kTheGamepadProduct = 0x0026;
const size_t kMaximumMenuGamepads = 2;
const size_t kMaximumMenuKeyboards = 4;
const unsigned int kMenuControllerBurstLimit = 12;
const int64_t kMenuControllerBurstWindowMs = 1000;
const int64_t kMenuControllerQuietResetMs = 1000;

volatile sig_atomic_t g_shutdown_requested = 0;

void signal_handler(int signal_number) {
  (void)signal_number;
  g_shutdown_requested = 1;
}

int64_t monotonic_ms() {
  struct timespec now;
  if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
    return 0;
  return static_cast<int64_t>(now.tv_sec) * 1000 + now.tv_nsec / 1000000;
}

std::string errno_message(const std::string &what) {
  return what + ": " + std::strerror(errno);
}

bool write_all(int fd, const char *data, size_t size) {
  while (size > 0) {
    const ssize_t written = write(fd, data, size);
    if (written > 0) {
      data += written;
      size -= static_cast<size_t>(written);
      continue;
    }
    if (written < 0 && errno == EINTR)
      continue;
    return false;
  }
  return true;
}

bool is_absolute_path(const std::string &path) {
  return !path.empty() && path[0] == '/' && path.size() < PATH_MAX;
}

std::string trim_ascii_space(const std::string &text) {
  size_t begin = 0;
  while (begin < text.size() &&
         std::isspace(static_cast<unsigned char>(text[begin])))
    ++begin;
  size_t end = text.size();
  while (end > begin &&
         std::isspace(static_cast<unsigned char>(text[end - 1])))
    --end;
  return text.substr(begin, end - begin);
}

bool valid_utf8_text(const std::string &text, size_t max_codepoints,
                     bool allow_empty) {
  if (text.empty())
    return allow_empty;

  size_t count = 0;
  for (size_t i = 0; i < text.size();) {
    const unsigned char first = static_cast<unsigned char>(text[i]);
    uint32_t codepoint = 0;
    size_t length = 0;
    if (first < 0x80) {
      codepoint = first;
      length = 1;
    } else if ((first & 0xe0) == 0xc0) {
      codepoint = first & 0x1f;
      length = 2;
    } else if ((first & 0xf0) == 0xe0) {
      codepoint = first & 0x0f;
      length = 3;
    } else if ((first & 0xf8) == 0xf0) {
      codepoint = first & 0x07;
      length = 4;
    } else {
      return false;
    }
    if (i + length > text.size())
      return false;
    for (size_t j = 1; j < length; ++j) {
      const unsigned char next = static_cast<unsigned char>(text[i + j]);
      if ((next & 0xc0) != 0x80)
        return false;
      codepoint = (codepoint << 6) | (next & 0x3f);
    }
    if ((length == 2 && codepoint < 0x80) ||
        (length == 3 && codepoint < 0x800) ||
        (length == 4 && codepoint < 0x10000) ||
        codepoint > 0x10ffff ||
        (codepoint >= 0xd800 && codepoint <= 0xdfff))
      return false;
    if (codepoint < 0x20 || codepoint == 0x7f)
      return false;
    ++count;
    if (count > max_codepoints)
      return false;
    i += length;
  }
  return true;
}

std::string display_ascii(const std::string &text) {
  std::string result;
  for (size_t i = 0; i < text.size();) {
    const unsigned char first = static_cast<unsigned char>(text[i]);
    if (first < 0x80) {
      result.push_back(static_cast<char>(first));
      ++i;
      continue;
    }
    size_t length = 1;
    if ((first & 0xe0) == 0xc0)
      length = 2;
    else if ((first & 0xf0) == 0xe0)
      length = 3;
    else if ((first & 0xf8) == 0xf0)
      length = 4;
    result.push_back('?');
    i += std::min(length, text.size() - i);
  }
  return result;
}

uint16_t rgb565(unsigned int red, unsigned int green, unsigned int blue) {
  return static_cast<uint16_t>(((red & 0xf8) << 8) |
                               ((green & 0xfc) << 3) | (blue >> 3));
}

struct RgbColor {
  unsigned int red;
  unsigned int green;
  unsigned int blue;

  uint16_t pixel() const { return rgb565(red, green, blue); }
};

RgbColor xterm_color(unsigned int index) {
  static const RgbColor ansi_colors[] = {
      {0, 0, 0},       {128, 0, 0},     {0, 128, 0},     {128, 128, 0},
      {0, 0, 128},     {128, 0, 128},   {0, 128, 128},   {192, 192, 192},
      {128, 128, 128}, {255, 0, 0},     {0, 255, 0},     {255, 255, 0},
      {0, 0, 255},     {255, 0, 255},   {0, 255, 255},   {255, 255, 255},
  };
  static const unsigned int cube_levels[] = {0, 95, 135, 175, 215, 255};

  if (index < 16)
    return ansi_colors[index];
  if (index < 232) {
    const unsigned int cube = index - 16;
    return RgbColor{cube_levels[cube / 36],
                    cube_levels[(cube / 6) % 6], cube_levels[cube % 6]};
  }
  if (index < 256) {
    const unsigned int level = 8 + (index - 232) * 10;
    return RgbColor{level, level, level};
  }
  return RgbColor{0, 0, 0};
}

bool is_xterm_color(const RgbColor &color) {
  for (unsigned int index = 0; index < 256; ++index) {
    const RgbColor candidate = xterm_color(index);
    if (candidate.red == color.red && candidate.green == color.green &&
        candidate.blue == color.blue)
      return true;
  }
  return false;
}

uint16_t color_pixel(const RgbColor &color) { return color.pixel(); }

// Every dashboard base color is a semantic 24-bit RGB value. The compiled
// palette can replace these defaults before any framebuffer is opened.
RgbColor kColorBackground = {0x00, 0x00, 0x00};
RgbColor kColorTextDark = {0x12, 0x12, 0x12};
RgbColor kColorField = {0x12, 0x12, 0x12};
RgbColor kColorSurface = {0x1c, 0x1c, 0x1c};
RgbColor kColorInactiveBorder = {0x5f, 0x5f, 0x5f};
RgbColor kColorControlBorder = {0x6c, 0x6c, 0x6c};
RgbColor kColorFooter = {0xbc, 0xbc, 0xbc};
RgbColor kColorInactiveText = {0xda, 0xda, 0xda};
RgbColor kColorText = {0xee, 0xee, 0xee};
RgbColor kColorWhite = {0xff, 0xff, 0xff};
RgbColor kColorTitle = {0xff, 0xff, 0xaf};
RgbColor kColorVolumeOff = {0xaf, 0x87, 0x87};
RgbColor kColorVolumeOn = {0x87, 0xaf, 0x87};
RgbColor kColorSelected = {0xec, 0xb6, 0xe7};
RgbColor kColorWifiActive = {0x5f, 0x87, 0xaf};
RgbColor kColorWifiFocus = {0x87, 0xaf, 0xff};
RgbColor kColorWifiActiveBorder = {0xaf, 0xaf, 0xff};
RgbColor kColorFieldLabel = {0xaf, 0xaf, 0xaf};
RgbColor kColorAccent = {0xfe, 0x6c, 0x27};
RgbColor kColorActive = {0x50, 0x33, 0x11};
RgbColor kColorControlSurface = {0x30, 0x30, 0x30};
RgbColor kColorMuted = {0x94, 0x94, 0x94};

struct SettingsIconDefinition {
  const char *name;
  int size;
  const char *rows[23];
};

const SettingsIconDefinition kSettingsIconDefinitions[] = {
    {"gear-classic", 9,
     {"..##.##..", ".#######.", "###...###", "##.....##",
      "##.....##", "##.....##", "###...###", ".#######.",
      "..##.##.."}},
    {"gear-square", 9,
     {".##...##.", ".##...##.", "#########", "##.....##",
      "##.....##", "##.....##", "#########", ".##...##.",
      ".##...##."}},
    {"gear-diamond", 9,
     {"....#....", "..#####..", ".##...##.", "##.....##",
      "#.......#", "##.....##", ".##...##.", "..#####..",
      "....#...."}},
    {"gear-eight", 9,
     {".##...##.", "###...###", ".#######.", "..#...#..",
      "..#...#..", "..#...#..", ".#######.", "###...###",
      ".##...##."}},
    {"gear-spoke", 9,
     {"...###...", ".#.###.#.", "..#####..", "###.#.###",
      "####.####", "###.#.###", "..#####..", ".#.###.#.",
      "...###..."}},
    {"gear-ring", 9,
     {"...###...", ".#######.", "###...###", "##.....##",
      "##.....##", "##.....##", "###...###", ".#######.",
      "...###..."}},
    {"gear-cross", 9,
     {"...###...", "...###...", "..#####..", "###...###",
      "###...###", "###...###", "..#####..", "...###...",
      "...###..."}},
    {"gear-compact", 9,
     {".........", "...###...", "..#####..", ".##...##.",
      ".##...##.", ".##...##.", "..#####..", "...###...",
      "........."}},
    {"gear-heavy", 9,
     {".###.###.", "#########", "###...###", "##.....##",
      "##.....##", "##.....##", "###...###", "#########",
      ".###.###."}},
    {"gear-rivet", 9,
     {"..#...#..", ".#######.", "##.#.#.##", ".#.....#.",
      ".#.....#.", ".#.....#.", "##.#.#.##", ".#######.",
      "..#...#.."}},
    {"gear-outline", 9,
     {"..##.##..", "..#...#..", "##.###.##", "#.#...#.#",
      "#.#...#.#", "#.#...#.#", "##.###.##", "..#...#..",
      "..##.##.."}},
    {"gear-steel-outline", 23,
     {".......................", ".......#.......#.......",
      ".......##.....##.......", ".......####.####.......",
      ".......#########.......", "......###########......",
      "......###.....###......", "..######.......######..",
      "..#####.........#####..", "...###...........###...",
      "....##...........##....", ".....#...........#.....",
      "....##...........##....", "...###...........###...",
      "..#####.........#####..", "..######.......######..",
      "......###.....###......", "......###########......",
      ".......#########.......", ".......####.####.......",
      ".......##.....##.......", ".......#.......#.......",
      "......................."}},
};

const size_t kLegacySettingsIconDefinitionCount =
    sizeof(kSettingsIconDefinitions) / sizeof(kSettingsIconDefinitions[0]);

struct KnekkoSettingsIconDefinition {
  const char *name;
  const char *filename;
  int size;
};

#include "knekko_settings_icons_generated.inc"

const size_t kKnekkoSettingsIconDefinitionCount =
    sizeof(kKnekkoSettingsIconDefinitions) /
    sizeof(kKnekkoSettingsIconDefinitions[0]);
const size_t kSettingsIconDefinitionCount =
    kLegacySettingsIconDefinitionCount +
    kKnekkoSettingsIconDefinitionCount;
const size_t kDefaultSettingsIcon = 11;
size_t gSettingsIcon = kDefaultSettingsIcon;

struct SettingsIconImage {
  int size;
  std::vector<uint16_t> pixels;
  std::vector<unsigned char> alpha;

  SettingsIconImage() : size(0) {}
  void clear() {
    size = 0;
    pixels.clear();
    alpha.clear();
  }
  bool loaded() const {
    return size > 0 &&
           pixels.size() == static_cast<size_t>(size * size) &&
           alpha.size() == pixels.size();
  }
};

SettingsIconImage gSettingsIconImage;

const char *settings_icon_name(size_t index) {
  if (index < kLegacySettingsIconDefinitionCount)
    return kSettingsIconDefinitions[index].name;
  index -= kLegacySettingsIconDefinitionCount;
  if (index < kKnekkoSettingsIconDefinitionCount)
    return kKnekkoSettingsIconDefinitions[index].name;
  return NULL;
}

bool settings_icon_index(const std::string &name, size_t *result) {
  for (size_t index = 0; index < kSettingsIconDefinitionCount; ++index) {
    const char *icon_name = settings_icon_name(index);
    if (icon_name && name == icon_name) {
      if (result)
        *result = index;
      return true;
    }
  }
  return false;
}

struct PaletteToken {
  const char *name;
  RgbColor *value;
  RgbColor default_value;
};

PaletteToken kPaletteTokens[] = {
    {"background", &kColorBackground, {0x00, 0x00, 0x00}},
    {"text-dark", &kColorTextDark, {0x12, 0x12, 0x12}},
    {"field", &kColorField, {0x12, 0x12, 0x12}},
    {"surface", &kColorSurface, {0x1c, 0x1c, 0x1c}},
    {"inactive-border", &kColorInactiveBorder, {0x5f, 0x5f, 0x5f}},
    {"control-border", &kColorControlBorder, {0x6c, 0x6c, 0x6c}},
    {"footer", &kColorFooter, {0xbc, 0xbc, 0xbc}},
    {"inactive-text", &kColorInactiveText, {0xda, 0xda, 0xda}},
    {"text", &kColorText, {0xee, 0xee, 0xee}},
    {"white", &kColorWhite, {0xff, 0xff, 0xff}},
    {"title", &kColorTitle, {0xff, 0xff, 0xaf}},
    {"volume-off", &kColorVolumeOff, {0xaf, 0x87, 0x87}},
    {"volume-on", &kColorVolumeOn, {0x87, 0xaf, 0x87}},
    {"selected", &kColorSelected, {0xec, 0xb6, 0xe7}},
    {"wifi-active", &kColorWifiActive, {0x5f, 0x87, 0xaf}},
    {"wifi-focus", &kColorWifiFocus, {0x87, 0xaf, 0xff}},
    {"wifi-active-border", &kColorWifiActiveBorder, {0xaf, 0xaf, 0xff}},
    {"field-label", &kColorFieldLabel, {0xaf, 0xaf, 0xaf}},
    {"accent", &kColorAccent, {0xfe, 0x6c, 0x27}},
    {"active", &kColorActive, {0x50, 0x33, 0x11}},
    {"control-surface", &kColorControlSurface, {0x30, 0x30, 0x30}},
    {"muted", &kColorMuted, {0x94, 0x94, 0x94}},
};

const size_t kPaletteTokenCount =
    sizeof(kPaletteTokens) / sizeof(kPaletteTokens[0]);

bool parse_color(const std::string &text, RgbColor *color) {
  if (!color || text.size() != 7 || text[0] != '#')
    return false;
  unsigned int value = 0;
  for (size_t i = 1; i < text.size(); ++i) {
    const char ch = text[i];
    unsigned int nibble = 0;
    if (ch >= '0' && ch <= '9')
      nibble = static_cast<unsigned int>(ch - '0');
    else if (ch >= 'a' && ch <= 'f')
      nibble = static_cast<unsigned int>(ch - 'a' + 10);
    else if (ch >= 'A' && ch <= 'F')
      nibble = static_cast<unsigned int>(ch - 'A' + 10);
    else
      return false;
    value = (value << 4) | nibble;
  }
  color->red = (value >> 16) & 0xff;
  color->green = (value >> 8) & 0xff;
  color->blue = value & 0xff;
  return true;
}

bool valid_system(const std::string &system) {
  return system == "nes" || system == "gb" || system == "gbc" ||
         system == "zx" || system == "chip8" || system == "deck";
}

bool read_exact_at(int fd, off_t offset, unsigned char *data, size_t size) {
  size_t used = 0;
  while (used < size) {
    const ssize_t amount = pread(fd, data + used, size - used,
                                 offset + static_cast<off_t>(used));
    if (amount > 0)
      used += static_cast<size_t>(amount);
    else if (amount < 0 && errno == EINTR)
      continue;
    else
      return false;
  }
  return true;
}

bool validate_rom(const std::string &system, const std::string &path,
                  std::string *error) {
  if (!valid_system(system)) {
    if (error)
      *error = "unsupported system: " + system;
    return false;
  }
  if (!is_absolute_path(path)) {
    if (error)
      *error = "game data path must be absolute: " + path;
    return false;
  }
  // Deck-native applications own any optional data path. Missing application
  // data must not prevent the launcher itself from booting.
  if (system == "deck")
    return true;

  const int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
  if (fd < 0) {
    if (error)
      *error = errno_message("cannot open game data " + path);
    return false;
  }

  struct stat info;
  bool ok = true;
  if (fstat(fd, &info) != 0) {
    if (error)
      *error = errno_message("cannot stat game data " + path);
    ok = false;
  } else if (!S_ISREG(info.st_mode)) {
    if (error)
      *error = "game data is not a regular file: " + path;
    ok = false;
  } else if (system == "nes") {
    unsigned char header[4] = {};
    if (info.st_size < 16 || !read_exact_at(fd, 0, header, sizeof(header)) ||
        std::memcmp(header, "NES\x1a", 4) != 0) {
      if (error)
        *error = "ROM has no iNES header: " + path;
      ok = false;
    }
  } else if (system == "gb" || system == "gbc") {
    static const unsigned char logo[48] = {
        0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b,
        0x03, 0x73, 0x00, 0x83, 0x00, 0x0c, 0x00, 0x0d,
        0x00, 0x08, 0x11, 0x1f, 0x88, 0x89, 0x00, 0x0e,
        0xdc, 0xcc, 0x6e, 0xe6, 0xdd, 0xdd, 0xd9, 0x99,
        0xbb, 0xbb, 0x67, 0x63, 0x6e, 0x0e, 0xec, 0xcc,
        0xdd, 0xdc, 0x99, 0x9f, 0xbb, 0xb9, 0x33, 0x3e};
    unsigned char header[0x50] = {};
    if (info.st_size < 0x150 || info.st_size > 8 * 1024 * 1024 ||
        !read_exact_at(fd, 0x100, header, sizeof(header)) ||
        std::memcmp(header + 4, logo, sizeof(logo)) != 0) {
      if (error)
        *error = "ROM has no valid Game Boy header: " + path;
      ok = false;
    } else {
      unsigned char checksum = 0;
      for (size_t i = 0x34; i <= 0x4c; ++i)
        checksum = static_cast<unsigned char>(checksum - header[i] - 1);
      const unsigned char cgb_flag = header[0x43];
      if (checksum != header[0x4d]) {
        if (error)
          *error = "ROM has an invalid Game Boy header checksum: " + path;
        ok = false;
      } else if (system == "gbc" && cgb_flag != 0x80 && cgb_flag != 0xc0) {
        if (error)
          *error = "GBC entry does not advertise color support: " + path;
        ok = false;
      }
    }
  } else if (system == "zx") {
    if (info.st_size < 4 || info.st_size > 8 * 1024 * 1024) {
      if (error)
        *error = "ZX Spectrum TAP must contain 4 bytes through 8 MiB: " + path;
      ok = false;
    } else {
      std::vector<unsigned char> tape(static_cast<size_t>(info.st_size));
      if (!read_exact_at(fd, 0, tape.data(), tape.size())) {
        if (error)
          *error = "cannot read complete ZX Spectrum TAP: " + path;
        ok = false;
      } else {
        size_t offset = 0;
        size_t block_count = 0;
        while (ok && offset < tape.size()) {
          if (tape.size() - offset < 2) {
            ok = false;
            break;
          }
          const size_t block_size =
              static_cast<size_t>(tape[offset]) |
              (static_cast<size_t>(tape[offset + 1]) << 8);
          offset += 2;
          if (block_size < 2 || block_size > tape.size() - offset) {
            ok = false;
            break;
          }
          unsigned char checksum = 0;
          for (size_t byte = 0; byte < block_size; ++byte)
            checksum ^= tape[offset + byte];
          if (checksum != 0) {
            ok = false;
            break;
          }
          offset += block_size;
          ++block_count;
        }
        if ((!ok || offset != tape.size() || block_count == 0) && error)
          *error = "ZX Spectrum TAP has invalid blocks or checksums: " + path;
      }
    }
  } else if (info.st_size < 1 || info.st_size > 65024) {
    if (error)
      *error = "CHIP-8 ROM must contain 1 through 65024 bytes: " + path;
    ok = false;
  }
  close(fd);
  return ok;
}

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

GameEntry built_in_terminal_entry(const std::string &launcher) {
  GameEntry entry;
  entry.id = "terminal";
  entry.title = "TERMINAL";
  entry.system = "deck";
  entry.rom = launcher;
  entry.color = xterm_color(67);
  return entry;
}

bool is_built_in_terminal(const GameEntry &game) {
  return game.id == "terminal" && game.system == "deck";
}

GameEntry built_in_lua_entry(const std::string &launcher) {
  GameEntry entry;
  entry.id = "lua-repl";
  entry.title = "LUA REPL";
  entry.system = "deck";
  entry.rom = launcher;
  entry.color = xterm_color(69);
  return entry;
}

bool is_built_in_lua(const GameEntry &game) {
  return game.id == "lua-repl" && game.system == "deck";
}

GameEntry built_in_lisp_entry(const std::string &launcher) {
  GameEntry entry;
  entry.id = "lisp-repl";
  entry.title = "LISP REPL";
  entry.system = "deck";
  entry.rom = launcher;
  entry.color = xterm_color(149);
  return entry;
}

bool is_built_in_lisp(const GameEntry &game) {
  return game.id == "lisp-repl" && game.system == "deck";
}

GameEntry built_in_python_entry(const std::string &launcher) {
  GameEntry entry;
  entry.id = "python-repl";
  entry.title = "PYTHON REPL";
  entry.system = "deck";
  entry.rom = launcher;
  entry.color = xterm_color(220);
  return entry;
}

bool is_built_in_python(const GameEntry &game) {
  return game.id == "python-repl" && game.system == "deck";
}

GameEntry built_in_scheme_entry(const std::string &launcher) {
  GameEntry entry;
  entry.id = "scheme-repl";
  entry.title = "SCHEME REPL";
  entry.system = "deck";
  entry.rom = launcher;
  entry.color = xterm_color(114);
  return entry;
}

bool is_built_in_scheme(const GameEntry &game) {
  return game.id == "scheme-repl" && game.system == "deck";
}

GameEntry built_in_chiptune_entry(const std::string &directory) {
  GameEntry entry;
  entry.id = "chiptunes";
  entry.title = "CHIPTUNES";
  entry.system = "deck";
  entry.rom = directory;
  entry.color = xterm_color(208);
  return entry;
}

bool is_built_in_chiptune(const GameEntry &game) {
  return game.id == "chiptunes" && game.system == "deck";
}

std::string terminal_mode_for_game(const GameEntry &game) {
  if (is_built_in_terminal(game))
    return "shell";
  if (is_built_in_lua(game))
    return "lua";
  if (is_built_in_lisp(game))
    return "lisp";
  if (is_built_in_python(game))
    return "python";
  if (is_built_in_scheme(game))
    return "scheme";
  return std::string();
}

std::string terminal_program_title(const std::string &mode) {
  if (mode == "lua")
    return "LUA REPL";
  if (mode == "lisp")
    return "LISP REPL";
  if (mode == "python")
    return "PYTHON REPL";
  if (mode == "scheme")
    return "SCHEME REPL";
  return "TERMINAL";
}

GameEntry built_in_reboot_entry(const std::string &executable) {
  GameEntry entry;
  entry.id = "reboot";
  entry.title = "REBOOT";
  entry.system = "deck";
  entry.rom = executable;
  entry.color = xterm_color(167);
  return entry;
}

bool is_built_in_reboot(const GameEntry &game) {
  return game.id == "reboot" && game.system == "deck";
}

bool reboot_confirmation_active(int64_t deadline, int64_t now) {
  return deadline > 0 && now < deadline;
}

std::vector<std::string> split_tabs(const std::string &line) {
  std::vector<std::string> fields;
  size_t start = 0;
  while (true) {
    const size_t tab = line.find('\t', start);
    if (tab == std::string::npos) {
      fields.push_back(line.substr(start));
      break;
    }
    fields.push_back(line.substr(start, tab - start));
    start = tab + 1;
  }
  return fields;
}

void reset_dashboard_palette() {
  for (size_t index = 0; index < kPaletteTokenCount; ++index)
    *kPaletteTokens[index].value = kPaletteTokens[index].default_value;
  gSettingsIcon = kDefaultSettingsIcon;
  gSettingsIconImage.clear();
}

bool load_dashboard_palette(const std::string &path, std::string *error) {
  if (!is_absolute_path(path)) {
    if (error)
      *error = "palette path must be absolute";
    return false;
  }
  struct stat path_info;
  if (lstat(path.c_str(), &path_info) != 0) {
    if (error)
      *error = errno_message("cannot stat palette " + path);
    return false;
  }
  if (!S_ISREG(path_info.st_mode) || path_info.st_size < 0 ||
      path_info.st_size > kMaximumPaletteBytes) {
    if (error)
      *error = "palette must be a regular file no larger than 4096 bytes";
    return false;
  }
  std::ifstream input(path.c_str(), std::ios::in | std::ios::binary);
  if (!input) {
    if (error)
      *error = errno_message("cannot open palette " + path);
    return false;
  }

  std::vector<RgbColor> values(kPaletteTokenCount, RgbColor{0, 0, 0});
  std::vector<bool> seen(kPaletteTokenCount, false);
  size_t settings_icon = kDefaultSettingsIcon;
  bool saw_settings_icon = false;
  std::string line;
  size_t line_number = 0;
  while (std::getline(input, line)) {
    ++line_number;
    if (!line.empty() && line[line.size() - 1] == '\r')
      line.erase(line.size() - 1);
    if (line.empty() || line[0] == '#')
      continue;
    const std::vector<std::string> fields = split_tabs(line);
    if (fields.size() != 2) {
      if (error)
        *error = "palette line " + std::to_string(line_number) +
                 " must have exactly 2 TSV fields";
      return false;
    }
    if (fields[0] == "settings-icon") {
      if (saw_settings_icon) {
        if (error)
          *error = "duplicate settings icon on line " +
                   std::to_string(line_number);
        return false;
      }
      if (!settings_icon_index(fields[1], &settings_icon)) {
        if (error)
          *error = "unknown settings icon on line " +
                   std::to_string(line_number);
        return false;
      }
      saw_settings_icon = true;
      continue;
    }
    size_t token = 0;
    while (token < kPaletteTokenCount &&
           fields[0] != kPaletteTokens[token].name)
      ++token;
    if (token == kPaletteTokenCount) {
      if (error)
        *error = "unknown palette role on line " +
                 std::to_string(line_number);
      return false;
    }
    if (seen[token]) {
      if (error)
        *error = "duplicate palette role on line " +
                 std::to_string(line_number);
      return false;
    }
    if (!parse_color(fields[1], &values[token])) {
      if (error)
        *error = "palette color on line " + std::to_string(line_number) +
                 " must have the form #RRGGBB";
      return false;
    }
    seen[token] = true;
  }
  if (input.bad()) {
    if (error)
      *error = "error while reading palette " + path;
    return false;
  }
  for (size_t token = 0; token < kPaletteTokenCount; ++token) {
    if (!seen[token]) {
      if (error)
        *error = "palette is missing role " +
                 std::string(kPaletteTokens[token].name);
      return false;
    }
  }
  for (size_t token = 0; token < kPaletteTokenCount; ++token)
    *kPaletteTokens[token].value = values[token];
  gSettingsIcon = settings_icon;
  return true;
}

struct NetworkStatus {
  std::string ssid;
  std::string wlan_ipv4;
  std::string wireguard_ipv4;
  std::string selector;

  bool operator==(const NetworkStatus &other) const {
    return ssid == other.ssid && wlan_ipv4 == other.wlan_ipv4 &&
           wireguard_ipv4 == other.wireguard_ipv4 &&
           selector == other.selector;
  }

  bool operator!=(const NetworkStatus &other) const {
    return !(*this == other);
  }
};

std::string interface_ipv4(const char *interface_name) {
  struct ifaddrs *addresses = NULL;
  if (getifaddrs(&addresses) != 0)
    return std::string();
  std::string result;
  for (const struct ifaddrs *entry = addresses; entry; entry = entry->ifa_next) {
    if (!entry->ifa_addr || !entry->ifa_name ||
        std::strcmp(entry->ifa_name, interface_name) != 0 ||
        entry->ifa_addr->sa_family != AF_INET)
      continue;
    char text[INET_ADDRSTRLEN] = {};
    const struct sockaddr_in *address =
        reinterpret_cast<const struct sockaddr_in *>(entry->ifa_addr);
    if (inet_ntop(AF_INET, &address->sin_addr, text, sizeof(text))) {
      result = text;
      break;
    }
  }
  freeifaddrs(addresses);
  return result;
}

std::string wireless_ssid(const char *interface_name) {
  const int socket_fd = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
  if (socket_fd < 0)
    return std::string();
  struct iwreq request;
  std::memset(&request, 0, sizeof(request));
  std::strncpy(request.ifr_name, interface_name, IFNAMSIZ - 1);
  char value[IW_ESSID_MAX_SIZE + 1] = {};
  request.u.essid.pointer = value;
  request.u.essid.length = IW_ESSID_MAX_SIZE;
  request.u.essid.flags = 0;
  const bool read = ioctl(socket_fd, SIOCGIWESSID, &request) == 0;
  close(socket_fd);
  if (!read)
    return std::string();
  const size_t length = std::min<size_t>(request.u.essid.length,
                                         IW_ESSID_MAX_SIZE);
  std::string ssid(value, value + length);
  while (!ssid.empty() && ssid[ssid.size() - 1] == '\0')
    ssid.erase(ssid.size() - 1);
  return valid_utf8_text(ssid, 32, true) ? display_ascii(ssid)
                                         : std::string("?");
}

std::string read_wifi_selector_status(const std::string &path) {
  if (!is_absolute_path(path))
    return "STATUS UNAVAILABLE";
  struct stat info;
  if (lstat(path.c_str(), &info) != 0 || !S_ISREG(info.st_mode) ||
      info.st_size < 1 || info.st_size > 128)
    return "STATUS UNAVAILABLE";
  std::ifstream input(path.c_str(), std::ios::in | std::ios::binary);
  std::string line;
  if (!input || !std::getline(input, line) || input.bad())
    return "STATUS UNAVAILABLE";
  if (!line.empty() && line[line.size() - 1] == '\r')
    line.erase(line.size() - 1);
  if (trim_ascii_space(line) != line || !valid_utf8_text(line, 64, false))
    return "STATUS INVALID";
  return display_ascii(line);
}

NetworkStatus read_network_status(const std::string &selector_status_path) {
  NetworkStatus status;
  status.ssid = wireless_ssid("wlan0");
  status.wlan_ipv4 = interface_ipv4("wlan0");
  status.wireguard_ipv4 = interface_ipv4("wg0");
  status.selector = read_wifi_selector_status(selector_status_path);
  return status;
}

bool valid_id(const std::string &id) {
  if (id.empty() || id.size() > 48)
    return false;
  for (size_t i = 0; i < id.size(); ++i) {
    const unsigned char ch = static_cast<unsigned char>(id[i]);
    const bool valid = std::islower(ch) || std::isdigit(ch) || ch == '-';
    if (!valid || (i == 0 && !std::islower(ch) && !std::isdigit(ch)))
      return false;
    if (ch == '-' && (i + 1 == id.size() ||
                      (i > 0 && id[i - 1] == '-')))
      return false;
  }
  return true;
}

bool is_optional_header(const std::vector<std::string> &fields) {
  return fields.size() == 5 && fields[0] == "id" &&
         fields[1] == "title" && fields[2] == "system" &&
         fields[3] == "rom" &&
         (fields[4] == "color" || fields[4] == "#RRGGBB");
}

bool load_manifest(const std::string &path, std::vector<GameEntry> *games,
                   std::string *error) {
  if (!games)
    return false;
  games->clear();
  if (!is_absolute_path(path)) {
    if (error)
      *error = "manifest path must be absolute";
    return false;
  }

  struct stat manifest_info;
  if (stat(path.c_str(), &manifest_info) != 0) {
    if (error)
      *error = errno_message("cannot stat manifest " + path);
    return false;
  }
  if (!S_ISREG(manifest_info.st_mode) || manifest_info.st_size < 0 ||
      manifest_info.st_size > kMaximumManifestBytes) {
    if (error)
      *error = "manifest must be a regular file no larger than 65536 bytes";
    return false;
  }

  std::ifstream input(path.c_str(), std::ios::in | std::ios::binary);
  if (!input) {
    if (error)
      *error = errno_message("cannot open manifest " + path);
    return false;
  }

  std::set<std::string> ids;
  std::set<std::string> roms;
  std::string line;
  size_t line_number = 0;
  bool saw_data = false;
  while (std::getline(input, line)) {
    ++line_number;
    if (line.size() > 4096) {
      if (error)
        *error = "manifest line " + std::to_string(line_number) +
                 " exceeds 4096 bytes";
      return false;
    }
    if (!line.empty() && line[line.size() - 1] == '\r')
      line.erase(line.size() - 1);
    if (line.empty() || line[0] == '#')
      continue;

    const std::vector<std::string> fields = split_tabs(line);
    if (!saw_data && is_optional_header(fields)) {
      saw_data = true;
      continue;
    }
    saw_data = true;
    if (fields.size() != 5) {
      if (error)
        *error = "manifest line " + std::to_string(line_number) +
                 " must have exactly 5 TSV fields";
      return false;
    }

    GameEntry game;
    game.id = fields[0];
    game.title = fields[1];
    game.system = fields[2];
    game.rom = fields[3];

    if (!valid_id(game.id)) {
      if (error)
        *error = "invalid id on manifest line " +
                 std::to_string(line_number);
      return false;
    }
    if (!valid_utf8_text(game.title, 64, false) ||
        trim_ascii_space(game.title) != game.title) {
      if (error)
        *error = "invalid title on manifest line " +
                 std::to_string(line_number);
      return false;
    }
    if (!valid_system(game.system)) {
      if (error)
        *error = "invalid system on manifest line " +
                 std::to_string(line_number);
      return false;
    }
    if (!valid_utf8_text(game.rom, PATH_MAX - 1, false) ||
        trim_ascii_space(game.rom) != game.rom) {
      if (error)
        *error = "invalid ROM path on manifest line " +
                 std::to_string(line_number);
      return false;
    }
    if (!parse_color(fields[4], &game.color)) {
      if (error)
        *error = "invalid #RRGGBB color on manifest line " +
                 std::to_string(line_number);
      return false;
    }
    if (!is_xterm_color(game.color)) {
      if (error)
        *error = "color is not in the xterm-256 palette on manifest line " +
                 std::to_string(line_number);
      return false;
    }
    std::string rom_error;
    if (!validate_rom(game.system, game.rom, &rom_error)) {
      if (error)
        *error = "manifest line " + std::to_string(line_number) + ": " +
                 rom_error;
      return false;
    }
    if (!ids.insert(game.id).second) {
      if (error)
        *error = "duplicate id on manifest line " +
                 std::to_string(line_number);
      return false;
    }
    if (!roms.insert(game.rom).second) {
      if (error)
        *error = "duplicate ROM on manifest line " +
                 std::to_string(line_number);
      return false;
    }

    games->push_back(game);
    if (games->size() > static_cast<size_t>(kMaxGames)) {
      if (error)
        *error = "manifest has more than " + std::to_string(kMaxGames) +
                 " games; use fewer entries to keep touch targets large";
      return false;
    }
  }

  if (input.bad()) {
    if (error)
      *error = "error while reading manifest " + path;
    return false;
  }
  if (games->empty()) {
    if (error)
      *error = "manifest contains no games";
    return false;
  }
  return true;
}

std::string parent_directory(const std::string &path) {
  const size_t slash = path.rfind('/');
  if (slash == std::string::npos)
    return ".";
  if (slash == 0)
    return "/";
  return path.substr(0, slash);
}

bool save_state_value(const std::string &path, const std::string &value,
                      const std::string &role, std::string *error) {
  if (!is_absolute_path(path)) {
    if (error)
      *error = role + " state path must be absolute";
    return false;
  }

  std::string temporary;
  int fd = -1;
  for (unsigned int attempt = 0; attempt < 16; ++attempt) {
    std::ostringstream name;
    name << path << ".tmp." << static_cast<long>(getpid()) << '.' << attempt;
    temporary = name.str();
    fd = open(temporary.c_str(), O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC,
              0600);
    if (fd >= 0)
      break;
    if (errno != EEXIST) {
      if (error)
        *error = errno_message("cannot create " + role +
                               " state temporary file");
      return false;
    }
  }
  if (fd < 0) {
    if (error)
      *error = "cannot allocate a " + role + " state temporary file";
    return false;
  }

  const std::string bytes = value + "\n";
  bool ok = write_all(fd, bytes.data(), bytes.size());
  if (!ok && error)
    *error = errno_message("cannot write " + role + " state");
  if (ok && fsync(fd) != 0) {
    ok = false;
    if (error)
      *error = errno_message("cannot sync " + role + " state");
  }
  if (close(fd) != 0 && ok) {
    ok = false;
    if (error)
      *error = errno_message("cannot close " + role + " state");
  }
  if (ok && rename(temporary.c_str(), path.c_str()) != 0) {
    ok = false;
    if (error)
      *error = errno_message("cannot replace " + role + " state");
  }
  if (!ok) {
    unlink(temporary.c_str());
    return false;
  }

  const std::string directory = parent_directory(path);
  const int directory_fd =
      open(directory.c_str(), O_RDONLY | O_DIRECTORY | O_CLOEXEC);
  if (directory_fd >= 0) {
    fsync(directory_fd);
    close(directory_fd);
  }
  return true;
}

bool load_state_value(const std::string &path, const std::string &role,
                      std::string *value, bool *missing, std::string *error) {
  if (!value || !missing || !is_absolute_path(path)) {
    if (error)
      *error = role + " state path must be absolute";
    return false;
  }
  value->clear();
  *missing = false;

  const int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
  if (fd < 0) {
    if (errno != ENOENT) {
      if (error)
        *error = errno_message("cannot open " + role + " state " + path);
      return false;
    }
    *missing = true;
    return true;
  }

  struct stat state_info;
  if (fstat(fd, &state_info) != 0) {
    const int saved_errno = errno;
    close(fd);
    errno = saved_errno;
    if (error)
      *error = errno_message("cannot stat " + role + " state " + path);
    return false;
  }
  if (!S_ISREG(state_info.st_mode) || state_info.st_size < 0 ||
      state_info.st_size > 64) {
    close(fd);
    if (error)
      *error = role +
               " state must be a regular file no larger than 64 bytes";
    return false;
  }

  char buffer[65] = {};
  size_t used = 0;
  bool read_failed = false;
  int saved_errno = 0;
  while (used < sizeof(buffer)) {
    const ssize_t amount = read(fd, buffer + used, sizeof(buffer) - used);
    if (amount > 0) {
      used += static_cast<size_t>(amount);
      continue;
    }
    if (amount == 0)
      break;
    if (errno == EINTR)
      continue;
    read_failed = true;
    saved_errno = errno;
    break;
  }
  close(fd);
  if (read_failed) {
    errno = saved_errno;
    if (error)
      *error = errno_message("cannot read " + role + " state " + path);
    return false;
  }
  if (used == sizeof(buffer)) {
    if (error)
      *error = role + " state is too large";
    return false;
  }
  value->assign(buffer, used);
  return true;
}

bool parse_volume_percent(const std::string &text, unsigned int *volume) {
  if (!volume || text.empty())
    return false;
  unsigned int value = 0;
  for (size_t index = 0; index < text.size(); ++index) {
    const char character = text[index];
    if (character < '0' || character > '9')
      return false;
    value = value * 10 + static_cast<unsigned int>(character - '0');
    if (value > 100)
      return false;
  }
  if (std::to_string(value) != text)
    return false;
  *volume = value;
  return true;
}

bool save_volume_state(const std::string &path, unsigned int volume,
                       std::string *error) {
  if (volume > 100) {
    if (error)
      *error = "volume must be between 0 and 100";
    return false;
  }
  return save_state_value(path, std::to_string(volume), "volume", error);
}

bool load_volume_state(const std::string &path, unsigned int default_volume,
                       unsigned int *volume, std::string *error) {
  if (!volume || default_volume > 100) {
    if (error)
      *error = "default volume must be between 0 and 100";
    return false;
  }
  std::string bytes;
  bool missing = false;
  if (!load_state_value(path, "volume", &bytes, &missing, error))
    return false;
  if (missing) {
    *volume = default_volume;
    return save_volume_state(path, *volume, error);
  }
  if (bytes == "on\n") {
    *volume = default_volume;
    return save_volume_state(path, *volume, error);
  }
  if (bytes == "off\n") {
    *volume = 0;
    return save_volume_state(path, *volume, error);
  }
  if (bytes.empty() || bytes[bytes.size() - 1] != '\n' ||
      bytes.find('\n') != bytes.size() - 1 ||
      !parse_volume_percent(bytes.substr(0, bytes.size() - 1), volume)) {
    if (error)
      *error = "volume state must contain a canonical integer from 0 through "
               "100 followed by a newline";
    return false;
  }
  return true;
}

bool read_device_integer(const std::string &path, const std::string &role,
                         unsigned int *value, std::string *error) {
  if (!value || !is_absolute_path(path)) {
    if (error)
      *error = role + " path must be absolute";
    return false;
  }
  const int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
  if (fd < 0) {
    if (error)
      *error = errno_message("cannot open " + role + " " + path);
    return false;
  }
  char buffer[64] = {};
  size_t used = 0;
  while (used < sizeof(buffer) - 1) {
    const ssize_t amount = read(fd, buffer + used, sizeof(buffer) - 1 - used);
    if (amount > 0) {
      used += static_cast<size_t>(amount);
      continue;
    }
    if (amount < 0 && errno == EINTR)
      continue;
    if (amount < 0 && error)
      *error = errno_message("cannot read " + role + " " + path);
    close(fd);
    if (amount < 0)
      return false;
    break;
  }
  close(fd);
  const std::string text = trim_ascii_space(std::string(buffer, used));
  if (text.empty()) {
    if (error)
      *error = role + " is empty";
    return false;
  }
  uint64_t parsed = 0;
  for (size_t index = 0; index < text.size(); ++index) {
    if (text[index] < '0' || text[index] > '9') {
      if (error)
        *error = role + " must contain an unsigned integer";
      return false;
    }
    parsed = parsed * 10 + static_cast<unsigned int>(text[index] - '0');
    if (parsed > UINT_MAX) {
      if (error)
        *error = role + " is too large";
      return false;
    }
  }
  *value = static_cast<unsigned int>(parsed);
  return true;
}

bool write_device_integer(const std::string &path, const std::string &role,
                          unsigned int value, std::string *error) {
  if (!is_absolute_path(path)) {
    if (error)
      *error = role + " path must be absolute";
    return false;
  }
  const int fd = open(path.c_str(), O_WRONLY | O_CLOEXEC);
  if (fd < 0) {
    if (error)
      *error = errno_message("cannot open " + role + " " + path);
    return false;
  }
  struct stat info;
  if (fstat(fd, &info) == 0 && S_ISREG(info.st_mode) && info.st_size > 0 &&
      ftruncate(fd, 0) != 0 && errno != EINVAL && errno != EPERM) {
    const int saved_errno = errno;
    close(fd);
    errno = saved_errno;
    if (error)
      *error = errno_message("cannot truncate " + role + " " + path);
    return false;
  }
  const std::string bytes = std::to_string(value) + "\n";
  const bool wrote = write_all(fd, bytes.data(), bytes.size());
  const int saved_errno = errno;
  const bool closed = close(fd) == 0;
  if (!wrote || !closed) {
    errno = saved_errno;
    if (error)
      *error = errno_message("cannot write " + role + " " + path);
    return false;
  }
  return true;
}

unsigned int brightness_raw_value(unsigned int percent,
                                  unsigned int maximum) {
  if (maximum == 0)
    return 0;
  const unsigned int raw =
      static_cast<unsigned int>((static_cast<uint64_t>(percent) * maximum + 50) /
                                100);
  return std::max(1U, std::min(maximum, raw));
}

bool set_brightness_percent(const std::string &brightness_path,
                            const std::string &state_path,
                            unsigned int maximum, unsigned int percent,
                            std::string *error) {
  if (maximum == 0 || percent < kMinimumBrightness || percent > 100 ||
      percent % kBrightnessStep != 0) {
    if (error)
      *error = "brightness must be a 10-point step from 10 through 100";
    return false;
  }
  if (!write_device_integer(brightness_path, "brightness",
                            brightness_raw_value(percent, maximum), error))
    return false;
  return save_state_value(state_path, std::to_string(percent), "brightness",
                          error);
}

bool load_brightness(const std::string &brightness_path,
                     const std::string &maximum_path,
                     const std::string &state_path, unsigned int *maximum,
                     unsigned int *percent, std::string *error) {
  if (!maximum || !percent)
    return false;
  unsigned int current = 0;
  if (!read_device_integer(maximum_path, "maximum brightness", maximum,
                           error) ||
      *maximum == 0 ||
      !read_device_integer(brightness_path, "brightness", &current, error) ||
      current > *maximum) {
    if (error && error->empty())
      *error = "brightness is outside the backlight range";
    return false;
  }

  std::string bytes;
  bool missing = false;
  if (!load_state_value(state_path, "brightness", &bytes, &missing, error))
    return false;
  if (missing) {
    const unsigned int observed = static_cast<unsigned int>(
        (static_cast<uint64_t>(current) * 100 + *maximum / 2) / *maximum);
    *percent = std::max(
        kMinimumBrightness,
        std::min(100U, ((observed + kBrightnessStep / 2) / kBrightnessStep) *
                           kBrightnessStep));
  } else {
    if (bytes.empty() || bytes[bytes.size() - 1] != '\n' ||
        bytes.find('\n') != bytes.size() - 1 ||
        !parse_volume_percent(bytes.substr(0, bytes.size() - 1), percent) ||
        *percent < kMinimumBrightness || *percent % kBrightnessStep != 0) {
      if (error)
        *error = "brightness state must contain a 10-point step from 10 "
                 "through 100 followed by a newline";
      return false;
    }
  }
  return set_brightness_percent(brightness_path, state_path, *maximum,
                                *percent, error);
}

bool valid_keymap(const std::string &keymap) {
  return keymap == "us" || keymap == "cz";
}

bool save_keymap_state(const std::string &path, const std::string &keymap,
                       std::string *error) {
  if (!valid_keymap(keymap)) {
    if (error)
      *error = "terminal keymap must be 'us' or 'cz'";
    return false;
  }
  return save_state_value(path, keymap, "terminal keymap", error);
}

bool load_keymap_state(const std::string &path, std::string *keymap,
                       std::string *error) {
  if (!keymap) {
    if (error)
      *error = "terminal keymap output is required";
    return false;
  }
  std::string bytes;
  bool missing = false;
  if (!load_state_value(path, "terminal keymap", &bytes, &missing, error))
    return false;
  if (missing) {
    *keymap = "us";
    return save_keymap_state(path, *keymap, error);
  }
  if (bytes == "us\n") {
    *keymap = "us";
    return true;
  }
  if (bytes == "cz\n") {
    *keymap = "cz";
    return true;
  }
  if (error)
    *error = "terminal keymap state must contain exactly 'us\\n' or 'cz\\n'";
  return false;
}

struct Rect {
  int x;
  int y;
  int width;
  int height;

  bool contains(int px, int py) const {
    return px >= x && py >= y && px < x + width && py < y + height;
  }
};

typedef std::vector<uint16_t> Canvas;

bool next_ppm_token(const std::vector<unsigned char> &bytes, size_t *offset,
                    std::string *token) {
  if (!offset || !token)
    return false;
  while (*offset < bytes.size()) {
    const unsigned char ch = bytes[*offset];
    if (std::isspace(ch)) {
      ++*offset;
      continue;
    }
    if (ch == '#') {
      while (*offset < bytes.size() && bytes[*offset] != '\n')
        ++*offset;
      continue;
    }
    break;
  }
  const size_t start = *offset;
  while (*offset < bytes.size() &&
         !std::isspace(static_cast<unsigned char>(bytes[*offset])) &&
         bytes[*offset] != '#')
    ++*offset;
  if (*offset == start || *offset - start > 32)
    return false;
  token->assign(reinterpret_cast<const char *>(&bytes[start]),
                *offset - start);
  return true;
}

bool parse_positive_dimension(const std::string &text, int maximum,
                              int *value) {
  if (!value || text.empty())
    return false;
  int parsed = 0;
  for (size_t index = 0; index < text.size(); ++index) {
    if (text[index] < '0' || text[index] > '9')
      return false;
    parsed = parsed * 10 + text[index] - '0';
    if (parsed > maximum)
      return false;
  }
  if (parsed < 1 || std::to_string(parsed) != text)
    return false;
  *value = parsed;
  return true;
}

bool load_ppm_cover_image(const std::string &path, CoverImage *cover,
                          std::string *error) {
  if (!cover)
    return false;
  *cover = CoverImage();
  const int fd = open(path.c_str(), O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
  if (fd < 0) {
    if (errno != ENOENT && error)
      *error = errno_message("cannot open cover " + path);
    return false;
  }

  struct stat info;
  if (fstat(fd, &info) != 0 || !S_ISREG(info.st_mode) || info.st_size < 12 ||
      info.st_size > kMaximumCoverBytes) {
    const int saved_errno = errno;
    close(fd);
    errno = saved_errno;
    if (error)
      *error = "cover must be a small regular PPM file: " + path;
    return false;
  }

  std::vector<unsigned char> bytes(static_cast<size_t>(info.st_size));
  size_t used = 0;
  while (used < bytes.size()) {
    const ssize_t amount = read(fd, &bytes[used], bytes.size() - used);
    if (amount > 0)
      used += static_cast<size_t>(amount);
    else if (amount < 0 && errno == EINTR)
      continue;
    else
      break;
  }
  const int read_errno = errno;
  const bool close_ok = close(fd) == 0;
  if (used != bytes.size() || !close_ok) {
    errno = read_errno;
    if (error)
      *error = errno_message("cannot read cover " + path);
    return false;
  }

  size_t offset = 0;
  std::string magic;
  std::string width_text;
  std::string height_text;
  std::string maximum_text;
  int width = 0;
  int height = 0;
  if (!next_ppm_token(bytes, &offset, &magic) || magic != "P6" ||
      !next_ppm_token(bytes, &offset, &width_text) ||
      !parse_positive_dimension(width_text, kMaximumCoverWidth, &width) ||
      !next_ppm_token(bytes, &offset, &height_text) ||
      !parse_positive_dimension(height_text, kMaximumCoverHeight, &height) ||
      !next_ppm_token(bytes, &offset, &maximum_text) ||
      maximum_text != "255" || offset >= bytes.size() ||
      !std::isspace(static_cast<unsigned char>(bytes[offset]))) {
    if (error)
      *error = "cover has an invalid P6 PPM header: " + path;
    return false;
  }
  if (bytes[offset] == '\r' && offset + 1 < bytes.size() &&
      bytes[offset + 1] == '\n')
    offset += 2;
  else
    ++offset;

  const size_t pixel_count = static_cast<size_t>(width * height);
  if (bytes.size() - offset != pixel_count * 3) {
    if (error)
      *error = "cover PPM pixel data has the wrong size: " + path;
    return false;
  }

  cover->pixels.reserve(pixel_count);
  for (size_t pixel = 0; pixel < pixel_count; ++pixel) {
    const size_t source = offset + pixel * 3;
    const RgbColor color{bytes[source], bytes[source + 1], bytes[source + 2]};
    if (!is_xterm_color(color)) {
      *cover = CoverImage();
      if (error)
        *error = "cover contains a color outside xterm-256: " + path;
      return false;
    }
    cover->pixels.push_back(color.pixel());
  }
  cover->width = width;
  cover->height = height;
  return true;
}

const std::vector<uint16_t> &xterm_quantization_table() {
  static std::vector<uint16_t> table;
  if (!table.empty())
    return table;
  table.resize(32 * 32 * 32);
  for (int red5 = 0; red5 < 32; ++red5) {
    const int red = (red5 << 3) | (red5 >> 2);
    for (int green5 = 0; green5 < 32; ++green5) {
      const int green = (green5 << 3) | (green5 >> 2);
      for (int blue5 = 0; blue5 < 32; ++blue5) {
        const int blue = (blue5 << 3) | (blue5 >> 2);
        unsigned int best_distance = UINT_MAX;
        uint16_t best_pixel = 0;
        for (unsigned int index = 0; index < 256; ++index) {
          const RgbColor candidate = xterm_color(index);
          const int red_delta = red - candidate.red;
          const int green_delta = green - candidate.green;
          const int blue_delta = blue - candidate.blue;
          const unsigned int distance = static_cast<unsigned int>(
              red_delta * red_delta + green_delta * green_delta +
              blue_delta * blue_delta);
          if (distance < best_distance) {
            best_distance = distance;
            best_pixel = candidate.pixel();
          }
        }
        const size_t offset = static_cast<size_t>((red5 << 10) |
                                                  (green5 << 5) | blue5);
        table[offset] = best_pixel;
      }
    }
  }
  return table;
}

bool load_png_cover_image(const std::string &path,
                          const RgbColor &background, CoverImage *cover,
                          std::string *error) {
  if (!cover)
    return false;
  *cover = CoverImage();
  struct stat info;
  if (lstat(path.c_str(), &info) != 0) {
    if (errno != ENOENT && error)
      *error = errno_message("cannot stat cover " + path);
    return false;
  }
  if (!S_ISREG(info.st_mode) || info.st_size < 8 ||
      info.st_size > kMaximumPngCoverBytes) {
    if (error)
      *error = "cover must be a regular PNG no larger than 4 MiB: " + path;
    return false;
  }

  png_image image;
  std::memset(&image, 0, sizeof(image));
  image.version = PNG_IMAGE_VERSION;
  if (!png_image_begin_read_from_file(&image, path.c_str())) {
    if (error)
      *error = "cannot decode cover PNG " + path + ": " + image.message;
    return false;
  }
  if (image.width < 1 || image.height < 1 ||
      image.width > kMaximumPngDimension ||
      image.height > kMaximumPngDimension) {
    png_image_free(&image);
    if (error)
      *error = "cover PNG dimensions are outside 1..2048: " + path;
    return false;
  }
  image.format = PNG_FORMAT_RGBA;
  std::vector<png_byte> source(PNG_IMAGE_SIZE(image));
  if (!png_image_finish_read(&image, NULL, &source[0], 0, NULL)) {
    if (error)
      *error = "cannot read cover PNG " + path + ": " + image.message;
    png_image_free(&image);
    return false;
  }

  png_uint_32 target_width = image.width;
  png_uint_32 target_height = image.height;
  if (target_width > static_cast<png_uint_32>(kMaximumCoverWidth) ||
      target_height > static_cast<png_uint_32>(kMaximumCoverHeight)) {
    if (static_cast<uint64_t>(target_width) * kMaximumCoverHeight >
        static_cast<uint64_t>(target_height) * kMaximumCoverWidth) {
      target_width = kMaximumCoverWidth;
      target_height = std::max<png_uint_32>(
          1, static_cast<png_uint_32>(static_cast<uint64_t>(image.height) *
                                      target_width / image.width));
    } else {
      target_height = kMaximumCoverHeight;
      target_width = std::max<png_uint_32>(
          1, static_cast<png_uint_32>(static_cast<uint64_t>(image.width) *
                                      target_height / image.height));
    }
  }

  const std::vector<uint16_t> &quantized = xterm_quantization_table();
  cover->pixels.resize(static_cast<size_t>(target_width * target_height));
  for (png_uint_32 y = 0; y < target_height; ++y) {
    const png_uint_32 source_y =
        static_cast<png_uint_32>(static_cast<uint64_t>(y) * image.height /
                                 target_height);
    for (png_uint_32 x = 0; x < target_width; ++x) {
      const png_uint_32 source_x =
          static_cast<png_uint_32>(static_cast<uint64_t>(x) * image.width /
                                   target_width);
      const size_t source_offset =
          static_cast<size_t>((source_y * image.width + source_x) * 4);
      const unsigned int alpha = source[source_offset + 3];
      const unsigned int inverse_alpha = 255 - alpha;
      const unsigned int red =
          (source[source_offset] * alpha + background.red * inverse_alpha +
           127) /
          255;
      const unsigned int green =
          (source[source_offset + 1] * alpha +
           background.green * inverse_alpha + 127) /
          255;
      const unsigned int blue =
          (source[source_offset + 2] * alpha + background.blue * inverse_alpha +
           127) /
          255;
      const size_t quantized_offset =
          static_cast<size_t>(((red >> 3) << 10) | ((green >> 3) << 5) |
                              (blue >> 3));
      cover->pixels[static_cast<size_t>(y * target_width + x)] =
          quantized[quantized_offset];
    }
  }
  cover->width = static_cast<int>(target_width);
  cover->height = static_cast<int>(target_height);
  png_image_free(&image);
  return true;
}

bool load_selected_settings_icon(const std::string &directory,
                                 std::string *error) {
  gSettingsIconImage.clear();
  if (gSettingsIcon < kLegacySettingsIconDefinitionCount)
    return true;
  if (!is_absolute_path(directory)) {
    if (error)
      *error = "settings icon directory must be an absolute path";
    return false;
  }
  const size_t definition_index =
      gSettingsIcon - kLegacySettingsIconDefinitionCount;
  if (definition_index >= kKnekkoSettingsIconDefinitionCount) {
    if (error)
      *error = "selected settings icon is outside the asset catalog";
    return false;
  }
  const KnekkoSettingsIconDefinition &definition =
      kKnekkoSettingsIconDefinitions[definition_index];
  const std::string path = directory + "/" + definition.filename;
  const int descriptor =
      open(path.c_str(), O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
  if (descriptor < 0) {
    if (error)
      *error = errno_message("cannot open settings icon " + path);
    return false;
  }
  struct stat info;
  if (fstat(descriptor, &info) != 0 || !S_ISREG(info.st_mode) ||
      info.st_size < 8 || info.st_size > 65536) {
    const int saved_errno = errno;
    close(descriptor);
    errno = saved_errno;
    if (error)
      *error = "settings icon must be a small regular PNG: " + path;
    return false;
  }
  FILE *file = fdopen(descriptor, "rb");
  if (!file) {
    const int saved_errno = errno;
    close(descriptor);
    errno = saved_errno;
    if (error)
      *error = errno_message("cannot read settings icon " + path);
    return false;
  }

  png_image image;
  std::memset(&image, 0, sizeof(image));
  image.version = PNG_IMAGE_VERSION;
  if (!png_image_begin_read_from_stdio(&image, file)) {
    if (error)
      *error = "cannot decode settings icon " + path + ": " + image.message;
    fclose(file);
    return false;
  }
  if (image.width != static_cast<png_uint_32>(definition.size) ||
      image.height != static_cast<png_uint_32>(definition.size)) {
    png_image_free(&image);
    fclose(file);
    if (error)
      *error = "settings icon dimensions do not match the catalog: " + path;
    return false;
  }
  image.format = PNG_FORMAT_RGBA;
  std::vector<png_byte> source(PNG_IMAGE_SIZE(image));
  if (!png_image_finish_read(&image, NULL, &source[0], 0, NULL)) {
    if (error)
      *error = "cannot read settings icon " + path + ": " + image.message;
    png_image_free(&image);
    fclose(file);
    return false;
  }
  png_image_free(&image);
  if (fclose(file) != 0) {
    if (error)
      *error = errno_message("cannot close settings icon " + path);
    return false;
  }

  const size_t pixel_count =
      static_cast<size_t>(definition.size * definition.size);
  gSettingsIconImage.size = definition.size;
  gSettingsIconImage.pixels.resize(pixel_count);
  gSettingsIconImage.alpha.resize(pixel_count);
  for (size_t pixel = 0; pixel < pixel_count; ++pixel) {
    const size_t offset = pixel * 4;
    const unsigned char alpha = source[offset + 3];
    gSettingsIconImage.alpha[pixel] = alpha;
    gSettingsIconImage.pixels[pixel] =
        RgbColor{source[offset], source[offset + 1], source[offset + 2]}
            .pixel();
  }
  return true;
}

size_t load_game_covers(const std::string &directory,
                        std::vector<GameEntry> *games) {
  if (!games || !is_absolute_path(directory))
    return 0;
  size_t loaded = 0;
  for (size_t index = 0; index < games->size(); ++index) {
    const std::string png_path =
        directory + "/" + (*games)[index].id + ".png";
    const std::string ppm_path =
        directory + "/" + (*games)[index].id + ".ppm";
    std::string error;
    CoverImage cover;
    if (load_png_cover_image(png_path, (*games)[index].color, &cover, &error) ||
        (error.empty() && load_ppm_cover_image(ppm_path, &cover, &error))) {
      (*games)[index].cover = cover;
      ++loaded;
    } else if (!error.empty()) {
      std::cerr << "deck-menu: " << error << std::endl;
    }
  }
  return loaded;
}

bool stage_canvas_for_scanout(const Canvas &canvas, size_t row_words,
                              std::vector<uint16_t> *frame) {
  if (!frame ||
      canvas.size() !=
          static_cast<size_t>(kLogicalWidth * kLogicalHeight) ||
      row_words < static_cast<size_t>(kPhysicalWidth) ||
      frame->size() < row_words * static_cast<size_t>(kPhysicalHeight))
    return false;

  for (int logical_x = 0; logical_x < kLogicalWidth; ++logical_x) {
    const int physical_row = kPhysicalHeight - 1 - logical_x;
    uint16_t *destination =
        &(*frame)[static_cast<size_t>(physical_row) * row_words];
    for (int logical_y = 0; logical_y < kLogicalHeight; ++logical_y) {
      destination[logical_y] =
          canvas[static_cast<size_t>(logical_y) * kLogicalWidth + logical_x];
    }
  }
  return true;
}

void fill_rect(Canvas *canvas, const Rect &rect, uint16_t color) {
  if (!canvas || canvas->size() !=
                     static_cast<size_t>(kLogicalWidth * kLogicalHeight))
    return;
  const int left = std::max(0, rect.x);
  const int top = std::max(0, rect.y);
  const int right = std::min(kLogicalWidth, rect.x + rect.width);
  const int bottom = std::min(kLogicalHeight, rect.y + rect.height);
  for (int y = top; y < bottom; ++y) {
    std::fill(canvas->begin() + y * kLogicalWidth + left,
              canvas->begin() + y * kLogicalWidth + right, color);
  }
}

void stroke_rect(Canvas *canvas, const Rect &rect, int thickness,
                 uint16_t color) {
  fill_rect(canvas, Rect{rect.x, rect.y, rect.width, thickness}, color);
  fill_rect(canvas,
            Rect{rect.x, rect.y + rect.height - thickness, rect.width,
                 thickness},
            color);
  fill_rect(canvas, Rect{rect.x, rect.y, thickness, rect.height}, color);
  fill_rect(canvas,
            Rect{rect.x + rect.width - thickness, rect.y, thickness,
                 rect.height},
            color);
}

void fill_pixel_cut_rect(Canvas *canvas, const Rect &rect, int cut,
                         uint16_t color) {
  if (rect.width <= cut * 2 || rect.height <= cut * 2)
    return;
  fill_rect(canvas,
            Rect{rect.x + cut, rect.y, rect.width - cut * 2, rect.height},
            color);
  fill_rect(canvas,
            Rect{rect.x, rect.y + cut, rect.width, rect.height - cut * 2},
            color);
}

void draw_pixel_panel(Canvas *canvas, const Rect &rect, uint16_t fill,
                      uint16_t border, int thickness = kPixelStroke) {
  fill_pixel_cut_rect(canvas, rect, thickness, border);
  const Rect inside{rect.x + thickness, rect.y + thickness,
                    rect.width - thickness * 2,
                    rect.height - thickness * 2};
  fill_pixel_cut_rect(canvas, inside, thickness, fill);
}

void draw_cover_square(Canvas *canvas, const Rect &bounds,
                       const CoverImage &cover) {
  if (!canvas || !cover.available() || bounds.width < 1 || bounds.height < 1)
    return;
  const int source_size = std::min(cover.width, cover.height);
  const int source_left = (cover.width - source_size) / 2;
  const int source_top = (cover.height - source_size) / 2;
  for (int y = 0; y < bounds.height; ++y) {
    const int source_y = source_top +
                         static_cast<int>(static_cast<int64_t>(y) *
                                          source_size / bounds.height);
    for (int x = 0; x < bounds.width; ++x) {
      const int source_x = source_left +
                           static_cast<int>(static_cast<int64_t>(x) *
                                            source_size / bounds.width);
      (*canvas)[static_cast<size_t>(bounds.y + y) * kLogicalWidth +
                bounds.x + x] =
          cover.pixels[static_cast<size_t>(source_y * cover.width + source_x)];
    }
  }
}

const uint8_t *glyph_rows(char input) {
  static const uint8_t space[7] = {0, 0, 0, 0, 0, 0, 0};
  static const uint8_t unknown[7] = {14, 17, 1, 2, 4, 0, 4};
  static const uint8_t digits[10][7] = {
      {14, 17, 19, 21, 25, 17, 14}, {4, 12, 4, 4, 4, 4, 14},
      {14, 17, 1, 2, 4, 8, 31},    {30, 1, 1, 14, 1, 1, 30},
      {2, 6, 10, 18, 31, 2, 2},    {31, 16, 16, 30, 1, 1, 30},
      {14, 16, 16, 30, 17, 17, 14}, {31, 1, 2, 4, 8, 8, 8},
      {14, 17, 17, 14, 17, 17, 14}, {14, 17, 17, 15, 1, 1, 14}};
  static const uint8_t letters[26][7] = {
      {14, 17, 17, 31, 17, 17, 17}, {30, 17, 17, 30, 17, 17, 30},
      {14, 17, 16, 16, 16, 17, 14}, {30, 17, 17, 17, 17, 17, 30},
      {31, 16, 16, 30, 16, 16, 31}, {31, 16, 16, 30, 16, 16, 16},
      {14, 17, 16, 23, 17, 17, 15}, {17, 17, 17, 31, 17, 17, 17},
      {14, 4, 4, 4, 4, 4, 14},      {7, 2, 2, 2, 18, 18, 12},
      {17, 18, 20, 24, 20, 18, 17}, {16, 16, 16, 16, 16, 16, 31},
      {17, 27, 21, 21, 17, 17, 17}, {17, 25, 21, 19, 17, 17, 17},
      {14, 17, 17, 17, 17, 17, 14}, {30, 17, 17, 30, 16, 16, 16},
      {14, 17, 17, 17, 21, 18, 13}, {30, 17, 17, 30, 20, 18, 17},
      {15, 16, 16, 14, 1, 1, 30},   {31, 4, 4, 4, 4, 4, 4},
      {17, 17, 17, 17, 17, 17, 14}, {17, 17, 17, 17, 17, 10, 4},
      {17, 17, 17, 17, 21, 21, 10}, {17, 17, 10, 4, 10, 17, 17},
      {17, 17, 10, 4, 4, 4, 4},     {31, 1, 2, 4, 8, 16, 31}};
  static const uint8_t lowercase[26][7] = {
      {0, 0, 14, 1, 15, 17, 15},    {16, 16, 30, 17, 17, 17, 30},
      {0, 0, 14, 16, 16, 17, 14},   {1, 1, 15, 17, 17, 17, 15},
      {0, 0, 14, 17, 31, 16, 14},   {6, 9, 8, 28, 8, 8, 8},
      {0, 0, 15, 17, 15, 1, 14},    {16, 16, 30, 17, 17, 17, 17},
      {4, 0, 12, 4, 4, 4, 14},      {2, 0, 6, 2, 2, 18, 12},
      {16, 16, 18, 20, 24, 20, 18}, {12, 4, 4, 4, 4, 4, 14},
      {0, 0, 26, 21, 21, 17, 17},   {0, 0, 30, 17, 17, 17, 17},
      {0, 0, 14, 17, 17, 17, 14},   {0, 0, 30, 17, 30, 16, 16},
      {0, 0, 15, 17, 15, 1, 1},     {0, 0, 22, 25, 16, 16, 16},
      {0, 0, 15, 16, 14, 1, 30},    {8, 8, 28, 8, 8, 9, 6},
      {0, 0, 17, 17, 17, 19, 13},   {0, 0, 17, 17, 17, 10, 4},
      {0, 0, 17, 17, 21, 21, 10},   {0, 0, 17, 10, 4, 10, 17},
      {0, 0, 17, 17, 15, 1, 14},    {0, 0, 31, 2, 4, 8, 31}};
  static const uint8_t period[7] = {0, 0, 0, 0, 0, 6, 6};
  static const uint8_t comma[7] = {0, 0, 0, 0, 6, 6, 4};
  static const uint8_t colon[7] = {0, 6, 6, 0, 6, 6, 0};
  static const uint8_t dash[7] = {0, 0, 0, 31, 0, 0, 0};
  static const uint8_t slash[7] = {1, 2, 2, 4, 8, 8, 16};
  static const uint8_t plus[7] = {0, 4, 4, 31, 4, 4, 0};
  static const uint8_t bang[7] = {4, 4, 4, 4, 4, 0, 4};
  static const uint8_t question[7] = {14, 17, 1, 2, 4, 0, 4};
  static const uint8_t apostrophe[7] = {4, 4, 8, 0, 0, 0, 0};
  static const uint8_t left_paren[7] = {2, 4, 8, 8, 8, 4, 2};
  static const uint8_t right_paren[7] = {8, 4, 2, 2, 2, 4, 8};
  static const uint8_t ampersand[7] = {12, 18, 20, 8, 21, 18, 13};
  static const uint8_t hash[7] = {10, 10, 31, 10, 31, 10, 10};
  static const uint8_t underscore[7] = {0, 0, 0, 0, 0, 0, 31};
  static const uint8_t semicolon[7] = {0, 6, 6, 0, 6, 6, 4};
  static const uint8_t equal[7] = {0, 31, 0, 31, 0, 0, 0};
  static const uint8_t quote[7] = {10, 10, 20, 0, 0, 0, 0};
  static const uint8_t star[7] = {0, 21, 14, 31, 14, 21, 0};
  static const uint8_t percent[7] = {25, 25, 2, 4, 8, 19, 19};
  static const uint8_t caret[7] = {4, 10, 17, 0, 0, 0, 0};
  static const uint8_t pipe[7] = {4, 4, 4, 4, 4, 4, 4};
  static const uint8_t backslash[7] = {16, 8, 8, 4, 2, 2, 1};
  static const uint8_t less[7] = {2, 4, 8, 16, 8, 4, 2};
  static const uint8_t greater[7] = {8, 4, 2, 1, 2, 4, 8};
  static const uint8_t left_bracket[7] = {14, 8, 8, 8, 8, 8, 14};
  static const uint8_t right_bracket[7] = {14, 2, 2, 2, 2, 2, 14};
  static const uint8_t left_brace[7] = {6, 4, 4, 24, 4, 4, 6};
  static const uint8_t right_brace[7] = {12, 4, 4, 3, 4, 4, 12};
  static const uint8_t at[7] = {14, 17, 23, 21, 23, 16, 14};
  static const uint8_t dollar[7] = {4, 15, 20, 14, 5, 30, 4};
  static const uint8_t backtick[7] = {8, 4, 0, 0, 0, 0, 0};
  static const uint8_t tilde[7] = {0, 0, 9, 22, 0, 0, 0};

  unsigned char ch = static_cast<unsigned char>(input);
  if (ch >= 'a' && ch <= 'z')
    return lowercase[ch - 'a'];
  if (ch >= 'A' && ch <= 'Z')
    return letters[ch - 'A'];
  if (ch >= '0' && ch <= '9')
    return digits[ch - '0'];
  switch (ch) {
  case ' ': return space;
  case '.': return period;
  case ',': return comma;
  case ':': return colon;
  case '-': return dash;
  case '/': return slash;
  case '+': return plus;
  case '!': return bang;
  case '?': return question;
  case '\'': return apostrophe;
  case '(': return left_paren;
  case ')': return right_paren;
  case '&': return ampersand;
  case '#': return hash;
  case '_': return underscore;
  case ';': return semicolon;
  case '=': return equal;
  case '"': return quote;
  case '*': return star;
  case '%': return percent;
  case '^': return caret;
  case '|': return pipe;
  case '\\': return backslash;
  case '<': return less;
  case '>': return greater;
  case '[': return left_bracket;
  case ']': return right_bracket;
  case '{': return left_brace;
  case '}': return right_brace;
  case '@': return at;
  case '$': return dollar;
  case '`': return backtick;
  case '~': return tilde;
  default: return unknown;
  }
}

void draw_character(Canvas *canvas, int x, int y, char ch, int scale,
                    uint16_t color) {
  const uint8_t *rows = glyph_rows(ch);
  for (int row = 0; row < 7; ++row) {
    for (int column = 0; column < 5; ++column) {
      if (rows[row] & (1u << (4 - column)))
        fill_rect(canvas,
                  Rect{x + column * scale, y + row * scale, scale, scale},
                  color);
    }
  }
}

int text_width(const std::string &text, int scale) {
  if (text.empty())
    return 0;
  return static_cast<int>(text.size()) * 6 * scale - scale;
}

void draw_text(Canvas *canvas, int x, int y, const std::string &utf8_text,
               int scale, uint16_t color) {
  const std::string text = display_ascii(utf8_text);
  for (size_t i = 0; i < text.size(); ++i)
    draw_character(canvas, x + static_cast<int>(i) * 6 * scale, y, text[i],
                   scale, color);
}

void draw_centered_text(Canvas *canvas, const Rect &bounds,
                        const std::string &text, int scale, uint16_t color) {
  const std::string shown = display_ascii(text);
  const int width = text_width(shown, scale);
  const int height = 7 * scale;
  draw_text(canvas, bounds.x + std::max(0, (bounds.width - width) / 2),
            bounds.y + std::max(0, (bounds.height - height) / 2), shown,
            scale, color);
}

int fit_text_scale(const std::string &text, int maximum_width, int preferred,
                   int minimum) {
  const std::string shown = display_ascii(text);
  for (int scale = preferred; scale >= minimum; --scale) {
    if (text_width(shown, scale) <= maximum_width)
      return scale;
  }
  return minimum;
}

std::string fit_text_width(const std::string &text, int maximum_width,
                           int scale) {
  std::string shown = display_ascii(text);
  if (text_width(shown, scale) <= maximum_width)
    return shown;
  const int character_width = 6 * scale;
  const size_t capacity = maximum_width > 0
                              ? static_cast<size_t>((maximum_width + scale) /
                                                    character_width)
                              : 0;
  if (capacity == 0)
    return std::string();
  if (capacity <= 3)
    return shown.substr(0, capacity);
  shown.resize(capacity - 3);
  shown += "...";
  return shown;
}

struct MenuLayout {
  Rect settings_button;
  Rect game_previous_button;
  Rect game_next_button;
  std::vector<std::string> systems;
  std::vector<Rect> system_buttons;
  std::vector<Rect> game_buttons;
  std::vector<Rect> game_position_indicators;
  std::vector<size_t> game_indices;
  std::vector<size_t> visible_game_indices;
  size_t shown_game_index;
};

enum MenuTarget {
  MenuTargetNone = -1,
  MenuTargetSettings = -2,
  MenuTargetGamePrevious = -3,
  MenuTargetGameNext = -4,
  MenuTargetSystemBase = 1000,
};

enum SettingsTarget {
  SettingsTargetNone = -1,
  SettingsTargetClose = 0,
  SettingsTargetVolumeDown,
  SettingsTargetVolumeUp,
  SettingsTargetBrightnessDown,
  SettingsTargetBrightnessUp,
  SettingsTargetTerminal,
  SettingsTargetKeymap,
  SettingsTargetWifi,
  SettingsTargetCount
};

struct SettingsLayout {
  Rect close_button;
  Rect wifi_button;
  Rect volume_down_button;
  Rect volume_up_button;
  Rect brightness_down_button;
  Rect brightness_up_button;
  Rect terminal_button;
  Rect keymap_button;
};

struct SystemDefinition {
  const char *system;
  const char *label;
};

const SystemDefinition kSystemDefinitions[] = {
    {"nes", "NES"},
    {"gb", "GAME BOY"},
    {"gbc", "GBC"},
    {"zx", "ZX SPECTRUM"},
    {"chip8", "CHIP-8"},
    {"deck", "DECK"},
};

bool has_system(const std::vector<GameEntry> &games,
                const std::string &system) {
  for (size_t i = 0; i < games.size(); ++i) {
    if (games[i].system == system)
      return true;
  }
  return false;
}

std::string initial_system(const std::vector<GameEntry> &games) {
  for (size_t definition = 0;
       definition < sizeof(kSystemDefinitions) / sizeof(kSystemDefinitions[0]);
       ++definition) {
    if (has_system(games, kSystemDefinitions[definition].system))
      return kSystemDefinitions[definition].system;
  }
  return games.empty() ? std::string() : games[0].system;
}

std::string system_label(const std::string &system) {
  for (size_t definition = 0;
       definition < sizeof(kSystemDefinitions) / sizeof(kSystemDefinitions[0]);
       ++definition) {
    if (system == kSystemDefinitions[definition].system)
      return kSystemDefinitions[definition].label;
  }
  return display_ascii(system);
}

void draw_terminal_icon(Canvas *canvas, const Rect &button, uint16_t color) {
  const int icon_height = 44;
  const int icon_top = button.y + (button.height - icon_height) / 2;
  const Rect screen{button.x + (button.width - 46) / 2, icon_top, 46, 34};
  stroke_rect(canvas, screen, 3, color);
  fill_rect(canvas, Rect{button.x + button.width / 2 - 3, icon_top + 34, 6, 7},
            color);
  fill_rect(canvas,
            Rect{button.x + 24, icon_top + 41, button.width - 48, 3},
            color);
  draw_text(canvas, screen.x + 7, screen.y + 9, ">_", 2, color);
}

enum ArrowDirection { ArrowUp, ArrowDown, ArrowLeft, ArrowRight };

void draw_wifi_icon(Canvas *canvas, const Rect &button, uint16_t color) {
  const int center_x = button.x + button.width / 2;
  const int top = button.y + 5;
  fill_rect(canvas, Rect{center_x - 6, top, 12, 5}, color);
  fill_rect(canvas, Rect{center_x - 12, top + 5, 6, 5}, color);
  fill_rect(canvas, Rect{center_x + 6, top + 5, 6, 5}, color);
  fill_rect(canvas, Rect{center_x - 18, top + 10, 6, 5}, color);
  fill_rect(canvas, Rect{center_x + 12, top + 10, 6, 5}, color);
  fill_rect(canvas, Rect{center_x - 6, top + 22, 12, 5}, color);
  fill_rect(canvas, Rect{center_x - 12, top + 27, 6, 5}, color);
  fill_rect(canvas, Rect{center_x + 6, top + 27, 6, 5}, color);
  fill_rect(canvas, Rect{center_x - 3, top + 36, 6, 6}, color);
}

void draw_outline_arrow(Canvas *canvas, const Rect &bounds,
                        ArrowDirection direction, uint16_t color) {
  const int center_x = bounds.x + bounds.width / 2;
  const int center_y = bounds.y + bounds.height / 2;
  const int mirror = direction == ArrowLeft ? -1 : 1;
  const auto block = [&](int x, int y, int width, int height) {
    const int left = mirror < 0 ? center_x - x - width : center_x + x;
    fill_rect(canvas, Rect{left, center_y + y, width, height}, color);
  };
  block(28, -2, 4, 4);
  block(24, -6, 4, 4);
  block(20, -10, 4, 4);
  block(16, -14, 4, 4);
  block(12, -18, 4, 4);
  block(8, -22, 4, 10);
  block(-28, -12, 36, 4);
  block(-28, -8, 4, 16);
  block(-28, 8, 36, 4);
  block(8, 12, 4, 10);
  block(12, 14, 4, 4);
  block(16, 10, 4, 4);
  block(20, 6, 4, 4);
  block(24, 2, 4, 4);
}

uint16_t blend_rgb565(uint16_t foreground, uint16_t background,
                      unsigned int alpha) {
  const unsigned int inverse = 255 - alpha;
  const unsigned int red =
      (((foreground >> 11) & 0x1f) * alpha +
       ((background >> 11) & 0x1f) * inverse + 127) /
      255;
  const unsigned int green =
      (((foreground >> 5) & 0x3f) * alpha +
       ((background >> 5) & 0x3f) * inverse + 127) /
      255;
  const unsigned int blue =
      ((foreground & 0x1f) * alpha + (background & 0x1f) * inverse + 127) /
      255;
  return static_cast<uint16_t>((red << 11) | (green << 5) | blue);
}

void draw_settings_icon(Canvas *canvas, const Rect &bounds, uint16_t color) {
  if (gSettingsIcon >= kLegacySettingsIconDefinitionCount &&
      gSettingsIconImage.loaded()) {
    const int target_size =
        std::max(1, std::min(50, std::min(bounds.width, bounds.height)));
    const int left = bounds.x + (bounds.width - target_size) / 2;
    const int top = bounds.y + (bounds.height - target_size) / 2;
    for (int y = 0; y < target_size; ++y) {
      const int source_y = y * gSettingsIconImage.size / target_size;
      for (int x = 0; x < target_size; ++x) {
        const int source_x = x * gSettingsIconImage.size / target_size;
        const size_t source = static_cast<size_t>(
            source_y * gSettingsIconImage.size + source_x);
        const unsigned int alpha = gSettingsIconImage.alpha[source];
        if (alpha != 0) {
          uint16_t &destination =
              (*canvas)[static_cast<size_t>(top + y) * kLogicalWidth + left +
                        x];
          destination =
              alpha == 255
                  ? gSettingsIconImage.pixels[source]
                  : blend_rgb565(gSettingsIconImage.pixels[source],
                                 destination, alpha);
        }
      }
    }
    return;
  }
  const size_t definition =
      gSettingsIcon < kLegacySettingsIconDefinitionCount
          ? gSettingsIcon
          : kDefaultSettingsIcon;
  const SettingsIconDefinition &icon = kSettingsIconDefinitions[definition];
  const int pixel =
      std::max(1, std::min(4, std::min(bounds.width / icon.size,
                                      bounds.height / icon.size)));
  const int left = bounds.x + (bounds.width - icon.size * pixel) / 2;
  const int top = bounds.y + (bounds.height - icon.size * pixel) / 2;
  for (int row = 0; row < icon.size; ++row) {
    for (int column = 0; column < icon.size; ++column) {
      if (icon.rows[row][column] == '#')
        fill_rect(canvas,
                  Rect{left + column * pixel, top + row * pixel, pixel, pixel},
                  color);
    }
  }
}

void draw_close_icon(Canvas *canvas, const Rect &bounds, uint16_t color) {
  const int center_x = bounds.x + bounds.width / 2;
  const int center_y = bounds.y + bounds.height / 2;
  for (int offset = -16; offset <= 16; offset += 4) {
    fill_rect(canvas, Rect{center_x + offset, center_y + offset, 4, 4}, color);
    fill_rect(canvas, Rect{center_x + offset, center_y - offset, 4, 4}, color);
  }
}

void draw_speaker_icon(Canvas *canvas, const Rect &bounds, bool loud,
                       uint16_t color) {
  const int x = bounds.x + 24;
  const int y = bounds.y + bounds.height / 2;
  fill_rect(canvas, Rect{x, y - 12, 12, 24}, color);
  fill_rect(canvas, Rect{x + 12, y - 20, 12, 40}, color);
  fill_rect(canvas, Rect{x + 24, y - 28, 8, 56}, color);
  fill_rect(canvas, Rect{x + 40, y - 16, 4, 32}, color);
  fill_rect(canvas, Rect{x + 44, y - 12, 4, 24}, color);
  if (loud) {
    fill_rect(canvas, Rect{x + 56, y - 24, 4, 48}, color);
    fill_rect(canvas, Rect{x + 60, y - 16, 4, 32}, color);
  }
}

void draw_sun_icon(Canvas *canvas, const Rect &bounds, bool bright,
                   uint16_t color) {
  const int center_x = bounds.x + bounds.width / 2;
  const int center_y = bounds.y + bounds.height / 2;
  const int half = bright ? 16 : 12;
  fill_pixel_cut_rect(canvas,
                      Rect{center_x - half, center_y - half, half * 2,
                           half * 2},
                      4, color);
  const int reach = bright ? 34 : 28;
  fill_rect(canvas, Rect{center_x - 3, center_y - reach, 6, 10}, color);
  fill_rect(canvas, Rect{center_x - 3, center_y + reach - 10, 6, 10}, color);
  fill_rect(canvas, Rect{center_x - reach, center_y - 3, 10, 6}, color);
  fill_rect(canvas, Rect{center_x + reach - 10, center_y - 3, 10, 6}, color);
  if (bright) {
    fill_rect(canvas, Rect{center_x - 25, center_y - 25, 7, 7}, color);
    fill_rect(canvas, Rect{center_x + 18, center_y - 25, 7, 7}, color);
    fill_rect(canvas, Rect{center_x - 25, center_y + 18, 7, 7}, color);
    fill_rect(canvas, Rect{center_x + 18, center_y + 18, 7, 7}, color);
  }
}

void draw_compact_power_icon(Canvas *canvas, const Rect &bounds,
                             uint16_t color) {
  const int center_x = bounds.x + bounds.width / 2;
  const int center_y = bounds.y + bounds.height / 2;
  fill_rect(canvas, Rect{center_x - 5, center_y - 58, 10, 54}, color);
  fill_rect(canvas, Rect{center_x - 48, center_y - 34, 22, 8}, color);
  fill_rect(canvas, Rect{center_x + 26, center_y - 34, 22, 8}, color);
  fill_rect(canvas, Rect{center_x - 58, center_y - 26, 8, 54}, color);
  fill_rect(canvas, Rect{center_x + 50, center_y - 26, 8, 54}, color);
  fill_rect(canvas, Rect{center_x - 48, center_y + 28, 16, 8}, color);
  fill_rect(canvas, Rect{center_x + 32, center_y + 28, 16, 8}, color);
  fill_rect(canvas, Rect{center_x - 32, center_y + 36, 64, 8}, color);
}

bool draw_compact_deck_logo(Canvas *canvas, const Rect &bounds,
                            const GameEntry &game) {
  if (game.system != "deck")
    return false;
  const uint16_t accent = game.color.pixel();
  if (game.id == "ten-seconds") {
    draw_centered_text(canvas, bounds, "10.00", 5, accent);
  } else if (is_built_in_lua(game)) {
    draw_pixel_panel(canvas,
                     Rect{bounds.x + 24, bounds.y + 46,
                          bounds.width - 48, bounds.height - 92},
                     color_pixel(kColorBackground), accent, 4);
    draw_centered_text(canvas, bounds, "LUA>", 4, accent);
  } else if (is_built_in_lisp(game)) {
    draw_centered_text(canvas, bounds, "(LISP)", 4, accent);
  } else if (is_built_in_python(game)) {
    draw_centered_text(canvas, bounds, ">>>", 6, accent);
    fill_rect(canvas,
              Rect{bounds.x + bounds.width / 2 - 54,
                   bounds.y + bounds.height / 2 + 34, 108, 6},
              accent);
  } else if (is_built_in_scheme(game)) {
    draw_centered_text(canvas, bounds, "(SCHEME)", 3, accent);
    fill_rect(canvas,
              Rect{bounds.x + bounds.width / 2 - 34,
                   bounds.y + bounds.height / 2 + 30, 68, 5},
              accent);
  } else if (is_built_in_chiptune(game)) {
    const int center_x = bounds.x + bounds.width / 2;
    const int center_y = bounds.y + bounds.height / 2;
    const int heights[9] = {34, 62, 92, 48, 112, 74, 42, 86, 56};
    for (int index = 0; index < 9; ++index) {
      fill_rect(canvas,
                Rect{center_x - 86 + index * 20,
                     center_y - heights[index] / 2, 10, heights[index]},
                accent);
    }
  } else if (is_built_in_terminal(game)) {
    const Rect screen{bounds.x + 30, bounds.y + 44, bounds.width - 60, 96};
    stroke_rect(canvas, screen, 4, accent);
    draw_centered_text(canvas, screen, ">_", 5, color_pixel(kColorText));
    fill_rect(canvas,
              Rect{bounds.x + bounds.width / 2 - 6, screen.y + screen.height,
                   12, 18},
              accent);
    fill_rect(canvas,
              Rect{bounds.x + bounds.width / 2 - 44,
                   screen.y + screen.height + 18, 88, 4},
              accent);
  } else if (is_built_in_reboot(game)) {
    draw_compact_power_icon(canvas, bounds, accent);
  } else {
    return false;
  }
  return true;
}

void draw_compact_cartridge(Canvas *canvas, const Rect &bounds,
                            uint16_t color) {
  const Rect cartridge{bounds.x + 34, bounds.y + 28, bounds.width - 68,
                       bounds.height - 56};
  draw_pixel_panel(canvas, cartridge, color_pixel(kColorBackground), color, 4);
  fill_rect(canvas,
            Rect{cartridge.x + 24, cartridge.y + 26,
                 cartridge.width - 48, 8},
            color);
  fill_rect(canvas,
            Rect{cartridge.x + 24, cartridge.y + 46,
                 cartridge.width - 48, 4},
            color);
  fill_rect(canvas,
            Rect{cartridge.x + 20, cartridge.y + cartridge.height - 30,
                 cartridge.width - 40, 10},
            color);
}

void draw_game_card(Canvas *canvas, const Rect &card, const GameEntry &game,
                    bool selected) {
  const uint16_t fill = selected ? color_pixel(kColorActive)
                                 : color_pixel(kColorBackground);
  draw_pixel_panel(canvas, card, fill, color_pixel(kColorAccent));
  const Rect art{card.x + 8, card.y + 8, card.width - 16, card.width - 16};
  if (game.cover.available()) {
    draw_cover_square(canvas, art, game.cover);
  } else if (!draw_compact_deck_logo(canvas, art, game)) {
    draw_compact_cartridge(canvas, art, game.color.pixel());
  }
  const Rect label{card.x + 8, card.y + card.width,
                   card.width - 16, card.height - card.width - 8};
  const std::string title =
      fit_text_width(game.title, label.width - 12, kGameTitleScale);
  draw_centered_text(canvas, label, title, kGameTitleScale,
                     color_pixel(kColorText));
}

void render_menu(const std::vector<GameEntry> &games,
                 const std::string &active_system, size_t game_position,
                 const std::string &status, Canvas *canvas,
                 MenuLayout *layout) {
  if (!canvas || !layout)
    return;
  canvas->assign(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                 color_pixel(kColorBackground));
  layout->settings_button = Rect{1212, 412, 56, 56};
  layout->game_previous_button = Rect{156, 232, 80, 100};
  layout->game_next_button = Rect{1044, 232, 80, 100};
  layout->systems.clear();
  layout->system_buttons.clear();
  layout->game_indices.clear();
  layout->visible_game_indices.clear();
  layout->game_buttons.clear();
  layout->game_position_indicators.clear();
  layout->shown_game_index = games.size();

  draw_settings_icon(canvas, layout->settings_button,
                     color_pixel(kColorFooter));

  for (size_t definition = 0;
       definition < sizeof(kSystemDefinitions) / sizeof(kSystemDefinitions[0]);
       ++definition) {
    if (has_system(games, kSystemDefinitions[definition].system))
      layout->systems.push_back(kSystemDefinitions[definition].system);
  }
  const int tab_gap = 8;
  const int tab_left = 56;
  const int tab_width = layout->systems.empty()
                            ? 0
                            : (1168 - tab_gap *
                                          (static_cast<int>(layout->systems.size()) - 1)) /
                                  static_cast<int>(layout->systems.size());
  for (size_t index = 0; index < layout->systems.size(); ++index) {
    const Rect tab{tab_left + static_cast<int>(index) * (tab_width + tab_gap),
                   76, tab_width, 52};
    layout->system_buttons.push_back(tab);
    const bool active = layout->systems[index] == active_system;
    draw_pixel_panel(canvas, tab,
                     active ? color_pixel(kColorActive)
                            : color_pixel(kColorBackground),
                     color_pixel(kColorAccent));
    const std::string label = system_label(layout->systems[index]);
    draw_centered_text(canvas, tab, label,
                       fit_text_scale(label, tab.width - 16, 2, 1),
                       color_pixel(kColorText));
  }

  for (size_t index = 0; index < games.size(); ++index) {
    if (games[index].system == active_system)
      layout->game_indices.push_back(index);
  }
  if (!layout->game_indices.empty()) {
    const size_t selected_position = game_position % layout->game_indices.size();
    layout->shown_game_index = layout->game_indices[selected_position];
    const size_t visible_count = std::min<size_t>(3, layout->game_indices.size());
    size_t first_position = 0;
    if (layout->game_indices.size() > visible_count) {
      if (selected_position == 0)
        first_position = 0;
      else if (selected_position + 1 >= layout->game_indices.size())
        first_position = layout->game_indices.size() - visible_count;
      else
        first_position = selected_position - 1;
    }
    const int card_width = 216;
    const int card_height = 264;
    const int card_gap = 36;
    const int row_width = static_cast<int>(visible_count) * card_width +
                          static_cast<int>(visible_count - 1) * card_gap;
    int card_x = (kLogicalWidth - row_width) / 2;
    for (size_t visible = 0; visible < visible_count; ++visible) {
      const size_t game_index =
          layout->game_indices[first_position + visible];
      const Rect card{card_x, 154, card_width, card_height};
      layout->visible_game_indices.push_back(game_index);
      layout->game_buttons.push_back(card);
      draw_game_card(canvas, card, games[game_index],
                     game_index == layout->shown_game_index);
      card_x += card_width + card_gap;
    }

    if (layout->game_indices.size() > 1) {
      draw_outline_arrow(canvas, layout->game_previous_button, ArrowLeft,
                         color_pixel(kColorFooter));
      draw_outline_arrow(canvas, layout->game_next_button, ArrowRight,
                         color_pixel(kColorFooter));
    } else {
      layout->game_previous_button = Rect{0, 0, 0, 0};
      layout->game_next_button = Rect{0, 0, 0, 0};
    }

    const int indicator_width = 16;
    const int indicator_height = 8;
    const int indicator_gap = 8;
    const int indicator_count = static_cast<int>(layout->game_indices.size());
    const int indicator_row_width =
        indicator_count * indicator_width +
        std::max(0, indicator_count - 1) * indicator_gap;
    int indicator_x = (kLogicalWidth - indicator_row_width) / 2;
    for (int indicator = 0; indicator < indicator_count; ++indicator) {
      const Rect bounds{indicator_x, 438, indicator_width, indicator_height};
      layout->game_position_indicators.push_back(bounds);
      stroke_rect(canvas, bounds, 2,
                  color_pixel(static_cast<size_t>(indicator) == selected_position
                                  ? kColorFooter
                                  : kColorControlBorder));
      indicator_x += indicator_width + indicator_gap;
    }
  }

  if (!status.empty()) {
    const int footer_scale = fit_text_scale(status, kLogicalWidth - 100, 2, 1);
    draw_centered_text(canvas, Rect{12, 452, kLogicalWidth - 100, 24}, status,
                       footer_scale, color_pixel(kColorFooter));
  }
}

void draw_settings_control(Canvas *canvas, const Rect &bounds, bool selected) {
  draw_pixel_panel(canvas, bounds,
                   selected ? color_pixel(kColorActive)
                            : color_pixel(kColorControlSurface),
                   selected ? color_pixel(kColorAccent)
                            : color_pixel(kColorControlBorder));
}

void render_settings(unsigned int volume, unsigned int brightness,
                     const std::string &keymap, int selected,
                     const std::string &status,
                     const NetworkStatus &network, Canvas *canvas,
                     SettingsLayout *layout) {
  if (!canvas || !layout)
    return;
  canvas->assign(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                 color_pixel(kColorBackground));
  layout->close_button = Rect{1212, 12, 56, 56};
  layout->wifi_button = Rect{926, 20, 262, 108};
  layout->volume_down_button = Rect{108, 208, 104, 104};
  layout->volume_up_button = Rect{228, 208, 104, 104};
  layout->brightness_down_button = Rect{438, 208, 104, 104};
  layout->brightness_up_button = Rect{558, 208, 104, 104};
  layout->terminal_button = Rect{792, 208, 112, 104};
  layout->keymap_button = Rect{1036, 208, 112, 104};

  draw_close_icon(canvas, layout->close_button, color_pixel(kColorText));
  const std::string active_ssid =
      network.ssid.empty() ? "NOT CONNECTED" : network.ssid;
  const std::string wlan_address =
      network.wlan_ipv4.empty() ? "NO ADDRESS" : network.wlan_ipv4;
  const std::string wireguard_address =
      network.wireguard_ipv4.empty() ? "NO ADDRESS" : network.wireguard_ipv4;
  draw_text(canvas, 64, 22, "ACTIVE WIFI", 1, color_pixel(kColorMuted));
  draw_text(canvas, 64, 44, fit_text_width(active_ssid, 300, 3), 3,
            color_pixel(kColorText));
  draw_text(canvas, 392, 22, "WLAN0", 1, color_pixel(kColorMuted));
  draw_text(canvas, 392, 44, wlan_address, 2, color_pixel(kColorText));
  draw_text(canvas, 620, 22, "WIREGUARD", 1, color_pixel(kColorMuted));
  draw_text(canvas, 620, 44, wireguard_address, 2,
            color_pixel(kColorText));
  draw_text(canvas, 64, 88,
            fit_text_width("AUTO WIFI: " + network.selector, 790, 1), 1,
            color_pixel(kColorFooter));
  draw_settings_control(canvas, layout->wifi_button,
                        selected == SettingsTargetWifi);
  draw_wifi_icon(canvas,
                 Rect{layout->wifi_button.x + 12, layout->wifi_button.y + 24,
                      54, 54},
                 color_pixel(kColorText));
  draw_text(canvas, layout->wifi_button.x + 78, layout->wifi_button.y + 28,
            "WIFI", 3, color_pixel(kColorText));
  draw_text(canvas, layout->wifi_button.x + 78, layout->wifi_button.y + 64,
            "SETTINGS", 2, color_pixel(kColorMuted));

  draw_settings_control(canvas, layout->volume_down_button,
                        selected == SettingsTargetVolumeDown);
  draw_settings_control(canvas, layout->volume_up_button,
                        selected == SettingsTargetVolumeUp);
  draw_speaker_icon(canvas, layout->volume_down_button, false,
                    color_pixel(kColorText));
  draw_speaker_icon(canvas, layout->volume_up_button, true,
                    color_pixel(kColorText));

  draw_settings_control(canvas, layout->brightness_down_button,
                        selected == SettingsTargetBrightnessDown);
  draw_settings_control(canvas, layout->brightness_up_button,
                        selected == SettingsTargetBrightnessUp);
  draw_sun_icon(canvas, layout->brightness_down_button, false,
                color_pixel(kColorText));
  draw_sun_icon(canvas, layout->brightness_up_button, true,
                color_pixel(kColorText));

  draw_settings_control(canvas, layout->terminal_button,
                        selected == SettingsTargetTerminal);
  draw_terminal_icon(canvas, layout->terminal_button,
                     color_pixel(kColorText));
  draw_settings_control(canvas, layout->keymap_button,
                        selected == SettingsTargetKeymap);
  draw_centered_text(canvas, layout->keymap_button,
                     keymap == "cz" ? "CZ" : "EN", 4,
                     color_pixel(kColorText));

  draw_centered_text(canvas, Rect{82, 328, 276, 34},
                     volume == 0 ? "OFF" : std::to_string(volume), 3,
                     color_pixel(kColorText));
  draw_centered_text(canvas, Rect{82, 366, 276, 28}, "VOLUME", 2,
                     color_pixel(kColorMuted));
  draw_centered_text(canvas, Rect{412, 328, 276, 34},
                     std::to_string(brightness), 3,
                     color_pixel(kColorText));
  draw_centered_text(canvas, Rect{412, 366, 276, 28}, "BRIGHTNESS", 2,
                     color_pixel(kColorMuted));
  draw_centered_text(canvas, Rect{750, 328, 196, 34}, "TERMINAL", 3,
                     color_pixel(kColorText));
  draw_centered_text(canvas, Rect{750, 366, 196, 28}, kTerminalLoginShell, 2,
                     color_pixel(kColorMuted));
  draw_centered_text(canvas, Rect{994, 328, 196, 34}, "KEYS", 3,
                     color_pixel(kColorText));
  draw_centered_text(canvas, Rect{994, 366, 196, 28},
                     keymap == "cz" ? "CZECH" : "US ANSI", 2,
                     color_pixel(kColorMuted));

  if (!status.empty()) {
    const int footer_scale = fit_text_scale(status, kLogicalWidth - 24, 2, 1);
    draw_centered_text(canvas, Rect{12, 440, kLogicalWidth - 24, 28}, status,
                       footer_scale, color_pixel(kColorFooter));
  }
}

enum WifiField { WifiSsid, WifiPassphrase };

struct WifiState {
  std::string ssid;
  std::string passphrase;
  WifiField field;
  bool uppercase;
  bool symbols;
  std::string status;

  WifiState() : field(WifiSsid), uppercase(false), symbols(false) {}
};

struct WifiKey {
  Rect bounds;
  char value;
};

struct WifiLayout {
  Rect back_button;
  Rect ssid_field;
  Rect passphrase_field;
  Rect save_button;
  Rect mode_button;
  Rect shift_button;
  Rect space_button;
  Rect delete_button;
  std::vector<WifiKey> keys;
};

std::string tail_for_field(const std::string &value, size_t maximum) {
  if (value.size() <= maximum)
    return value;
  if (maximum <= 3)
    return value.substr(value.size() - maximum);
  return "..." + value.substr(value.size() - (maximum - 3));
}

void draw_wifi_button(Canvas *canvas, const Rect &bounds,
                      const std::string &label, bool active) {
  const uint16_t background = color_pixel(active ? kColorWifiActive
                                                 : kColorSurface);
  fill_rect(canvas, bounds, background);
  stroke_rect(canvas, bounds, 3,
              color_pixel(active ? kColorWifiActiveBorder
                                 : kColorControlBorder));
  const int scale = fit_text_scale(label, bounds.width - 12, 3, 1);
  draw_centered_text(canvas, bounds, label, scale, color_pixel(kColorWhite));
}

void add_wifi_key_row(Canvas *canvas, const std::string &values, int y,
                      bool uppercase, WifiLayout *layout) {
  const int gap = 6;
  const int margin = 16;
  const int count = static_cast<int>(values.size());
  const int width = (kLogicalWidth - 2 * margin - gap * (count - 1)) / count;
  const int used = count * width + gap * (count - 1);
  const int left = (kLogicalWidth - used) / 2;
  for (int index = 0; index < count; ++index) {
    char value = values[static_cast<size_t>(index)];
    if (uppercase && value >= 'a' && value <= 'z')
      value = static_cast<char>(value - 'a' + 'A');
    const Rect bounds{left + index * (width + gap), y, width, 62};
    WifiKey key{bounds, value};
    layout->keys.push_back(key);
    draw_wifi_button(canvas, bounds, std::string(1, value), false);
  }
}

void render_wifi(const WifiState &state, const NetworkStatus &network,
                 Canvas *canvas, WifiLayout *layout) {
  if (!canvas || !layout)
    return;
  canvas->assign(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                 color_pixel(kColorBackground));
  layout->keys.clear();
  layout->back_button = Rect{16, 10, 120, 62};
  layout->ssid_field = Rect{330, 10, 310, 62};
  layout->passphrase_field = Rect{650, 10, 330, 62};
  layout->save_button = Rect{990, 10, 274, 62};
  draw_wifi_button(canvas, layout->back_button, "BACK", false);
  draw_text(canvas, 158, 25, "ADD WIFI", 3, color_pixel(kColorTitle));

  const uint16_t field_bg = color_pixel(kColorField);
  fill_rect(canvas, layout->ssid_field, field_bg);
  stroke_rect(canvas, layout->ssid_field, 3,
              color_pixel(state.field == WifiSsid ? kColorWifiFocus
                                                  : kColorInactiveBorder));
  draw_text(canvas, layout->ssid_field.x + 10, layout->ssid_field.y + 7,
            "SSID", 1, color_pixel(kColorFieldLabel));
  draw_text(canvas, layout->ssid_field.x + 10, layout->ssid_field.y + 28,
            tail_for_field(state.ssid, 19), 2, color_pixel(kColorWhite));

  fill_rect(canvas, layout->passphrase_field, field_bg);
  stroke_rect(canvas, layout->passphrase_field, 3,
              color_pixel(state.field == WifiPassphrase
                              ? kColorWifiFocus
                              : kColorInactiveBorder));
  draw_text(canvas, layout->passphrase_field.x + 10,
            layout->passphrase_field.y + 7, "PASSWORD", 1,
            color_pixel(kColorFieldLabel));
  draw_text(canvas, layout->passphrase_field.x + 10,
            layout->passphrase_field.y + 28,
            tail_for_field(std::string(state.passphrase.size(), '*'), 20), 2,
            color_pixel(kColorWhite));
  draw_wifi_button(canvas, layout->save_button, "SAVE NETWORK", false);

  if (state.symbols) {
    add_wifi_key_row(canvas, "1234567890", 86, false, layout);
    add_wifi_key_row(canvas, "!@#$%^&*()", 154, false, layout);
    add_wifi_key_row(canvas, "-_=+[]{}\\|", 222, false, layout);
    add_wifi_key_row(canvas, "`~;:'\",./?<>", 290, false, layout);
  } else {
    add_wifi_key_row(canvas, "qwertyuiop", 86, state.uppercase, layout);
    add_wifi_key_row(canvas, "asdfghjkl", 154, state.uppercase, layout);
    add_wifi_key_row(canvas, "zxcvbnm", 222, state.uppercase, layout);
    add_wifi_key_row(canvas, "@._-", 290, false, layout);
  }

  layout->mode_button = Rect{16, 364, 152, 66};
  layout->shift_button = Rect{176, 364, 168, 66};
  layout->space_button = Rect{352, 364, 700, 66};
  layout->delete_button = Rect{1060, 364, 204, 66};
  draw_wifi_button(canvas, layout->mode_button, state.symbols ? "ABC" : "123",
                   state.symbols);
  draw_wifi_button(canvas, layout->shift_button,
                   state.uppercase ? "ABC" : "abc",
                   !state.symbols && state.uppercase);
  draw_wifi_button(canvas, layout->space_button, "SPACE", false);
  draw_wifi_button(canvas, layout->delete_button, "DELETE", false);

  const std::string footer = state.status.empty()
                                 ? "SAVING DOES NOT INTERRUPT CURRENT WIFI"
                                 : state.status;
  draw_centered_text(canvas, Rect{12, 436, kLogicalWidth - 24, 10}, footer, 1,
                     color_pixel(kColorFooter));
  const std::string active_ssid =
      network.ssid.empty() ? "NOT CONNECTED" : network.ssid;
  const std::string wlan_address =
      network.wlan_ipv4.empty() ? "NO ADDRESS" : network.wlan_ipv4;
  const std::string wireguard_address =
      network.wireguard_ipv4.empty() ? "NO ADDRESS" : network.wireguard_ipv4;
  const std::string addresses =
      "WIFI " + active_ssid + "  WLAN0 " + wlan_address + "  WG0 " +
      wireguard_address;
  draw_centered_text(canvas, Rect{12, 450, kLogicalWidth - 24, 10},
                     fit_text_width(addresses, kLogicalWidth - 32, 1), 1,
                     color_pixel(kColorText));
  draw_centered_text(canvas, Rect{12, 464, kLogicalWidth - 24, 10},
                     fit_text_width("AUTO WIFI: " + network.selector,
                                    kLogicalWidth - 32, 1),
                     1, color_pixel(kColorMuted));
}

struct TouchReport {
  int x;
  int y;
  bool down;
  bool pressed;
  bool released;
};

class Framebuffer {
public:
  Framebuffer()
      : fd_(-1), memory_(NULL), map_size_(0), stride_(0)
#ifdef RETRO_DECK_WAYLAND
        , wayland_(NULL)
#endif
  {}
  ~Framebuffer() { close_device(); }

  bool open_device(std::string *error) {
#ifdef RETRO_DECK_WAYLAND
    if (wayland_ && wayland_->is_open())
      return true;
#endif
    close_device();
#ifdef RETRO_DECK_WAYLAND
    const char *wayland_display = std::getenv("WAYLAND_DISPLAY");
    if (wayland_display && wayland_display[0]) {
      wayland_ = new DeckWaylandPresentation;
      std::string wayland_error;
      if (wayland_->open_widget(&wayland_error))
        return true;
      std::cerr << "deck-menu: Wayland widget unavailable: " << wayland_error
                << "; trying fbdev" << std::endl;
      delete wayland_;
      wayland_ = NULL;
    }
#endif
    fd_ = open("/dev/fb0", O_RDWR | O_CLOEXEC);
    if (fd_ < 0) {
      if (error)
        *error = errno_message("cannot open /dev/fb0");
      return false;
    }

    struct fb_var_screeninfo variable;
    struct fb_fix_screeninfo fixed;
    std::memset(&variable, 0, sizeof(variable));
    std::memset(&fixed, 0, sizeof(fixed));
    if (ioctl(fd_, FBIOGET_VSCREENINFO, &variable) != 0 ||
        ioctl(fd_, FBIOGET_FSCREENINFO, &fixed) != 0) {
      if (error)
        *error = errno_message("cannot query framebuffer geometry");
      close_device();
      return false;
    }

    const unsigned int rows =
        variable.yres_virtual ? variable.yres_virtual : variable.yres;
    if (fixed.line_length == 0 ||
        rows > std::numeric_limits<size_t>::max() / fixed.line_length) {
      if (error)
        *error = "framebuffer geometry overflows the address space";
      close_device();
      return false;
    }
    const size_t required = static_cast<size_t>(fixed.line_length) * rows;
    if (variable.xres != kPhysicalWidth ||
        variable.yres != kPhysicalHeight || variable.bits_per_pixel != 16 ||
        variable.xoffset != 0 || variable.yoffset != 0 ||
        rows < kPhysicalHeight || fixed.type != FB_TYPE_PACKED_PIXELS ||
        fixed.visual != FB_VISUAL_TRUECOLOR || fixed.line_length > INT_MAX ||
        fixed.line_length < kPhysicalWidth * 2 ||
        (fixed.line_length & 1) != 0 || fixed.smem_len < required ||
        variable.red.offset != 11 || variable.red.length != 5 ||
        variable.red.msb_right != 0 || variable.green.offset != 5 ||
        variable.green.length != 6 || variable.green.msb_right != 0 ||
        variable.blue.offset != 0 || variable.blue.length != 5 ||
        variable.blue.msb_right != 0 || variable.transp.length != 0) {
      if (error)
        *error = "unsupported framebuffer; expected 600x1280 RGB565 with a "
                 "valid stride";
      close_device();
      return false;
    }

    stride_ = fixed.line_length;
    map_size_ = fixed.smem_len;
    memory_ = static_cast<unsigned char *>(
        mmap(NULL, map_size_, PROT_READ | PROT_WRITE, MAP_SHARED, fd_, 0));
    if (memory_ == MAP_FAILED) {
      memory_ = NULL;
      if (error)
        *error = errno_message("cannot mmap /dev/fb0");
      close_device();
      return false;
    }
    frame_.assign(map_size_ / sizeof(uint16_t), 0);
    if (ioctl(fd_, FBIOBLANK, FB_BLANK_UNBLANK) != 0 && errno != EINVAL &&
        errno != ENOTTY) {
      std::cerr << "deck-menu: warning: cannot unblank framebuffer: "
                << std::strerror(errno) << std::endl;
    }
    return true;
  }

  bool present(const Canvas &canvas, std::string *error) {
#ifdef RETRO_DECK_WAYLAND
    if (wayland_)
      return wayland_->present_rgb565(
          &canvas[0], kLogicalWidth, kLogicalHeight,
          static_cast<size_t>(kLogicalWidth) * sizeof(uint16_t), error);
#endif
    if (!memory_ ||
        canvas.size() !=
            static_cast<size_t>(kLogicalWidth * kLogicalHeight)) {
      if (error)
        *error = "framebuffer or logical canvas is not initialized";
      return false;
    }

    const size_t row_words =
        static_cast<size_t>(stride_) / sizeof(uint16_t);
    if (!stage_canvas_for_scanout(canvas, row_words, &frame_)) {
      if (error)
        *error = "framebuffer staging buffer is unavailable";
      return false;
    }

    // Build the complete rotated image in cacheable RAM before touching live
    // scanout. Publishing finished rows avoids the visible black intermediate
    // frame caused by clearing framebuffer memory between menu screens.
    const size_t active_row_bytes =
        static_cast<size_t>(kLogicalHeight) * sizeof(uint16_t);
    for (int physical_row = 0; physical_row < kPhysicalHeight;
         ++physical_row) {
      const size_t offset = static_cast<size_t>(physical_row) * row_words;
      std::memcpy(memory_ + static_cast<size_t>(physical_row) * stride_,
                  &frame_[offset], active_row_bytes);
    }
    return true;
  }

  void close_device() {
#ifdef RETRO_DECK_WAYLAND
    delete wayland_;
    wayland_ = NULL;
#endif
    if (memory_) {
      munmap(memory_, map_size_);
      memory_ = NULL;
    }
    if (fd_ >= 0) {
      close(fd_);
      fd_ = -1;
    }
    map_size_ = 0;
    stride_ = 0;
    frame_.clear();
  }

  void close_for_child() {
#ifdef RETRO_DECK_WAYLAND
    if (wayland_)
      return;
#endif
    close_device();
  }

  bool uses_wayland() const {
#ifdef RETRO_DECK_WAYLAND
    return wayland_ != NULL;
#else
    return false;
#endif
  }

  int input_fd() const {
#ifdef RETRO_DECK_WAYLAND
    return wayland_ ? wayland_->fd() : -1;
#else
    return -1;
#endif
  }

  bool read_wayland_touch(std::vector<TouchReport> *reports,
                           std::string *error) {
#ifdef RETRO_DECK_WAYLAND
    if (!wayland_ || !reports)
      return false;
    if (!wayland_->dispatch(error))
      return false;
    std::vector<DeckWaylandTouchReport> wayland_reports;
    wayland_->take_touch_reports(&wayland_reports);
    reports->clear();
    for (size_t index = 0; index < wayland_reports.size(); ++index) {
      TouchReport report;
      report.x = wayland_reports[index].x;
      report.y = wayland_reports[index].y;
      report.down = wayland_reports[index].down;
      report.pressed = wayland_reports[index].pressed;
      report.released = wayland_reports[index].released;
      reports->push_back(report);
    }
    if (wayland_->shutdown_requested())
      g_shutdown_requested = 1;
    return true;
#else
    (void)reports;
    (void)error;
    return false;
#endif
  }

private:
  int fd_;
  unsigned char *memory_;
  size_t map_size_;
  int stride_;
  std::vector<uint16_t> frame_;
#ifdef RETRO_DECK_WAYLAND
  DeckWaylandPresentation *wayland_;
#endif
};

bool bit_is_set(const unsigned long *bits, unsigned int bit) {
  const unsigned int bits_per_word = sizeof(unsigned long) * CHAR_BIT;
  return (bits[bit / bits_per_word] &
          (1UL << (bit % bits_per_word))) != 0;
}

class TouchDevice {
public:
  TouchDevice()
      : fd_(-1), x_(0), y_(0), current_down_(false),
        reported_down_(false), dropping_events_(false), grabbed_(false) {}
  ~TouchDevice() { close_device(); }

  int fd() const { return fd_; }
  int x() const { return x_; }
  int y() const { return y_; }
  bool down() const { return reported_down_; }

  bool discover(std::string *error) {
    close_device();
    DIR *directory = opendir("/dev/input");
    if (!directory) {
      if (error)
        *error = errno_message("cannot open /dev/input");
      return false;
    }

    std::vector<std::string> paths;
    for (struct dirent *entry = readdir(directory); entry;
         entry = readdir(directory)) {
      const std::string name(entry->d_name);
      if (name.size() <= 5 || name.compare(0, 5, "event") != 0)
        continue;
      bool numeric = true;
      for (size_t i = 5; i < name.size(); ++i)
        numeric = numeric && std::isdigit(static_cast<unsigned char>(name[i]));
      if (numeric)
        paths.push_back("/dev/input/" + name);
    }
    closedir(directory);
    std::sort(paths.begin(), paths.end());

    std::string rejected_reason;
    for (size_t i = 0; i < paths.size(); ++i) {
      const int candidate = open(paths[i].c_str(), O_RDONLY | O_NONBLOCK |
                                                       O_CLOEXEC);
      if (candidate < 0)
        continue;
      char name[256] = {};
      if (ioctl(candidate, EVIOCGNAME(sizeof(name)), name) < 0 ||
          std::string(name).find("Goodix Capacitive TouchScreen") ==
              std::string::npos) {
        close(candidate);
        continue;
      }

      struct input_absinfo x_info;
      struct input_absinfo y_info;
      std::memset(&x_info, 0, sizeof(x_info));
      std::memset(&y_info, 0, sizeof(y_info));
      const size_t key_words = (KEY_MAX + sizeof(unsigned long) * CHAR_BIT) /
                               (sizeof(unsigned long) * CHAR_BIT);
      std::vector<unsigned long> keys(key_words, 0);
      if (ioctl(candidate, EVIOCGABS(ABS_X), &x_info) != 0 ||
          ioctl(candidate, EVIOCGABS(ABS_Y), &y_info) != 0 ||
          ioctl(candidate, EVIOCGBIT(EV_KEY,
                                     keys.size() * sizeof(unsigned long)),
                &keys[0]) < 0 ||
          !bit_is_set(&keys[0], BTN_TOUCH) || x_info.minimum != 0 ||
          x_info.maximum != kLogicalWidth - 1 || y_info.minimum != 0 ||
          y_info.maximum != kLogicalHeight - 1) {
        rejected_reason =
            "Goodix device has unexpected ABS_X/ABS_Y/BTN_TOUCH capabilities";
        close(candidate);
        continue;
      }

      fd_ = candidate;
      x_ = std::max(0, std::min(kLogicalWidth - 1, x_info.value));
      y_ = std::max(0, std::min(kLogicalHeight - 1, y_info.value));
      current_down_ = false;
      reported_down_ = false;
      dropping_events_ = false;
      std::fill(keys.begin(), keys.end(), 0);
      if (ioctl(fd_, EVIOCGKEY(keys.size() * sizeof(unsigned long)),
                &keys[0]) >= 0) {
        current_down_ = bit_is_set(&keys[0], BTN_TOUCH);
        reported_down_ = current_down_;
      }
      if (ioctl(fd_, EVIOCGRAB, 1) == 0) {
        grabbed_ = true;
      } else {
        std::cerr << "deck-menu: warning: cannot exclusively grab " << paths[i]
                  << ": " << std::strerror(errno) << std::endl;
      }
      return true;
    }

    if (error) {
      *error = rejected_reason.empty()
                   ? "Goodix Capacitive TouchScreen was not found"
                   : rejected_reason;
    }
    return false;
  }

  bool read_reports(std::vector<TouchReport> *reports, std::string *error) {
    if (!reports || fd_ < 0)
      return false;
    reports->clear();
    while (true) {
      struct input_event events[32];
      const ssize_t amount = read(fd_, events, sizeof(events));
      if (amount < 0) {
        if (errno == EINTR)
          continue;
        if (errno == EAGAIN || errno == EWOULDBLOCK)
          return true;
        if (error)
          *error = errno_message("touchscreen read failed");
        return false;
      }
      if (amount == 0) {
        if (error)
          *error = "touchscreen disconnected";
        return false;
      }
      if (amount % static_cast<ssize_t>(sizeof(struct input_event)) != 0) {
        if (error)
          *error = "touchscreen returned a partial input_event";
        return false;
      }

      const size_t count = static_cast<size_t>(amount) / sizeof(events[0]);
      for (size_t i = 0; i < count; ++i) {
        const struct input_event &event = events[i];
        if (dropping_events_) {
          if (event.type == EV_SYN && event.code == SYN_REPORT) {
            resynchronize();
            TouchReport report;
            report.x = x_;
            report.y = y_;
            report.down = current_down_;
            report.pressed = current_down_ && !reported_down_;
            report.released = !current_down_ && reported_down_;
            reports->push_back(report);
            reported_down_ = current_down_;
            dropping_events_ = false;
          }
          continue;
        }
        if (event.type == EV_SYN && event.code == SYN_DROPPED) {
          dropping_events_ = true;
          continue;
        }
        if (event.type == EV_ABS && event.code == ABS_X) {
          x_ = std::max(0, std::min(kLogicalWidth - 1, event.value));
        } else if (event.type == EV_ABS && event.code == ABS_Y) {
          y_ = std::max(0, std::min(kLogicalHeight - 1, event.value));
        } else if (event.type == EV_KEY && event.code == BTN_TOUCH) {
          current_down_ = event.value != 0;
        } else if (event.type == EV_SYN && event.code == SYN_REPORT) {
          TouchReport report;
          report.x = x_;
          report.y = y_;
          report.down = current_down_;
          report.pressed = current_down_ && !reported_down_;
          report.released = !current_down_ && reported_down_;
          reports->push_back(report);
          reported_down_ = current_down_;
        }
      }
    }
  }

  void close_for_child() const {
    if (fd_ >= 0)
      close(fd_);
  }

  void close_device() {
    if (fd_ >= 0) {
      if (grabbed_)
        ioctl(fd_, EVIOCGRAB, 0);
      close(fd_);
    }
    fd_ = -1;
    grabbed_ = false;
    current_down_ = false;
    reported_down_ = false;
    dropping_events_ = false;
  }

private:
  void resynchronize() {
    struct input_absinfo info;
    if (ioctl(fd_, EVIOCGABS(ABS_X), &info) == 0)
      x_ = std::max(0, std::min(kLogicalWidth - 1, info.value));
    if (ioctl(fd_, EVIOCGABS(ABS_Y), &info) == 0)
      y_ = std::max(0, std::min(kLogicalHeight - 1, info.value));

    const size_t key_words = (KEY_MAX + sizeof(unsigned long) * CHAR_BIT) /
                             (sizeof(unsigned long) * CHAR_BIT);
    std::vector<unsigned long> keys(key_words, 0);
    if (ioctl(fd_, EVIOCGKEY(keys.size() * sizeof(unsigned long)), &keys[0]) >=
        0)
      current_down_ = bit_is_set(&keys[0], BTN_TOUCH);
  }

  int fd_;
  int x_;
  int y_;
  bool current_down_;
  bool reported_down_;
  bool dropping_events_;
  bool grabbed_;
};

unsigned int menu_gamepad_key_to_button(unsigned short code) {
  switch (code) {
  case BTN_THUMB2:
  case BTN_TOP:
    return kMenuPadConfirm;
  case BTN_THUMB:
  case BTN_TRIGGER:
    return kMenuPadBack;
  case BTN_TOP2:
    return kMenuPadSystemPrevious;
  case BTN_PINKIE:
    return kMenuPadSystemNext;
  case BTN_BASE:
    return kMenuPadSettings;
  default:
    return 0;
  }
}

unsigned int menu_keyboard_key_to_button(unsigned short code, bool shift) {
  switch (code) {
  case KEY_ENTER:
  case KEY_KPENTER:
    return kMenuPadConfirm;
  case KEY_ESC:
    return kMenuPadBack;
  case KEY_UP:
    return kMenuPadUp;
  case KEY_DOWN:
    return kMenuPadDown;
  case KEY_LEFT:
    return kMenuPadLeft;
  case KEY_RIGHT:
    return kMenuPadRight;
  case KEY_TAB:
    return shift ? kMenuPadSystemPrevious : kMenuPadSystemNext;
  default:
    return 0;
  }
}

bool menu_keyboard_key_repeats(unsigned short code) {
  return code == KEY_UP || code == KEY_DOWN || code == KEY_LEFT ||
         code == KEY_RIGHT;
}

unsigned int menu_keyboard_event_to_button(unsigned short code, int value,
                                           bool *left_shift,
                                           bool *right_shift) {
  if (!left_shift || !right_shift)
    return 0;
  if (code == KEY_LEFTSHIFT) {
    *left_shift = value != 0;
    return 0;
  }
  if (code == KEY_RIGHTSHIFT) {
    *right_shift = value != 0;
    return 0;
  }
  const bool fresh_press = value == 1;
  const bool allowed_repeat = value == 2 && menu_keyboard_key_repeats(code);
  if (!fresh_press && !allowed_repeat)
    return 0;
  return menu_keyboard_key_to_button(code, *left_shift || *right_shift);
}

bool menu_keyboard_capabilities(const unsigned long *keys) {
  if (!keys)
    return false;
  return bit_is_set(keys, KEY_ENTER) && bit_is_set(keys, KEY_ESC) &&
         bit_is_set(keys, KEY_TAB) && bit_is_set(keys, KEY_UP) &&
         bit_is_set(keys, KEY_DOWN) && bit_is_set(keys, KEY_LEFT) &&
         bit_is_set(keys, KEY_RIGHT) &&
         (bit_is_set(keys, KEY_LEFTSHIFT) ||
          bit_is_set(keys, KEY_RIGHTSHIFT));
}

unsigned int menu_gamepad_axis_to_button(int value, int minimum, int maximum,
                                         unsigned int negative,
                                         unsigned int positive) {
  if (maximum <= minimum)
    return 0;
  const int64_t span = static_cast<int64_t>(maximum) - minimum;
  const int low = minimum + static_cast<int>(span / 3);
  const int high = maximum - static_cast<int>(span / 3);
  if (value <= low)
    return negative;
  if (value >= high)
    return positive;
  return 0;
}

struct MenuGamepadDevice {
  int fd;
  std::string path;
  std::string physical_path;
  struct input_absinfo x_info;
  struct input_absinfo y_info;
  int x_value;
  int y_value;
  uint32_t raw_buttons;
  unsigned int state;
  bool dropping_events;

  MenuGamepadDevice()
      : fd(-1), x_value(0), y_value(0), raw_buttons(0), state(0),
        dropping_events(false) {
    std::memset(&x_info, 0, sizeof(x_info));
    std::memset(&y_info, 0, sizeof(y_info));
  }
};

unsigned int menu_gamepad_state(const MenuGamepadDevice &gamepad) {
  unsigned int state = 0;
  for (unsigned int index = 0; index < 8; ++index) {
    if (gamepad.raw_buttons & (1u << index)) {
      state |= menu_gamepad_key_to_button(
          static_cast<unsigned short>(BTN_TRIGGER + index));
    }
  }
  state |= menu_gamepad_axis_to_button(
      gamepad.x_value, gamepad.x_info.minimum, gamepad.x_info.maximum,
      kMenuPadLeft, kMenuPadRight);
  state |= menu_gamepad_axis_to_button(
      gamepad.y_value, gamepad.y_info.minimum, gamepad.y_info.maximum,
      kMenuPadUp, kMenuPadDown);
  return state;
}

class MenuGamepads {
public:
  MenuGamepads() : last_scan_ms_(0) {}
  ~MenuGamepads() { close_devices(); }

  size_t count() const {
    size_t connected = 0;
    for (size_t i = 0; i < devices_.size(); ++i)
      connected += devices_[i].fd >= 0 ? 1 : 0;
    return connected;
  }

  bool scan_if_due(bool force, std::string *error) {
    const int64_t now = monotonic_ms();
    if (!force && last_scan_ms_ > 0 && now - last_scan_ms_ < 1000)
      return true;
    last_scan_ms_ = now;
    return scan(error);
  }

  void append_poll_descriptors(std::vector<struct pollfd> *descriptors) const {
    if (!descriptors)
      return;
    for (size_t i = 0; i < devices_.size(); ++i) {
      struct pollfd descriptor;
      descriptor.fd = devices_[i].fd;
      descriptor.events = POLLIN;
      descriptor.revents = 0;
      descriptors->push_back(descriptor);
    }
  }

  unsigned int read_ready(const std::vector<struct pollfd> &descriptors,
                          size_t first_descriptor) {
    unsigned int pressed = 0;
    for (size_t i = 0; i < devices_.size(); ++i) {
      if (first_descriptor + i >= descriptors.size())
        break;
      const short revents = descriptors[first_descriptor + i].revents;
      if (!(revents & (POLLIN | POLLERR | POLLHUP | POLLNVAL)))
        continue;
      if ((revents & POLLIN) && drain_device(&devices_[i], &pressed))
        continue;
      close(devices_[i].fd);
      devices_[i].fd = -1;
      devices_[i].state = 0;
      devices_[i].raw_buttons = 0;
      last_scan_ms_ = 0;
    }
    return pressed;
  }

  void close_for_child() {
    close_devices();
    last_scan_ms_ = 0;
  }

private:
  bool scan(std::string *error) {
    DIR *directory = opendir("/dev/input");
    if (!directory) {
      if (error)
        *error = errno_message("cannot scan gamepads");
      return false;
    }

    std::vector<MenuGamepadDevice> candidates;
    for (struct dirent *entry = readdir(directory); entry;
         entry = readdir(directory)) {
      const std::string name(entry->d_name);
      if (name.size() <= 5 || name.compare(0, 5, "event") != 0)
        continue;
      bool numeric = true;
      for (size_t i = 5; i < name.size(); ++i)
        numeric = numeric && std::isdigit(static_cast<unsigned char>(name[i]));
      if (!numeric)
        continue;

      MenuGamepadDevice candidate;
      candidate.path = "/dev/input/" + name;
      candidate.fd = open(candidate.path.c_str(),
                          O_RDONLY | O_NONBLOCK | O_CLOEXEC);
      if (candidate.fd < 0)
        continue;

      struct input_id identity;
      std::memset(&identity, 0, sizeof(identity));
      if (ioctl(candidate.fd, EVIOCGID, &identity) != 0 ||
          identity.vendor != kTheGamepadVendor ||
          identity.product != kTheGamepadProduct ||
          ioctl(candidate.fd, EVIOCGABS(ABS_X), &candidate.x_info) != 0 ||
          ioctl(candidate.fd, EVIOCGABS(ABS_Y), &candidate.y_info) != 0) {
        close(candidate.fd);
        continue;
      }

      char physical_path[PATH_MAX] = {};
      if (ioctl(candidate.fd, EVIOCGPHYS(sizeof(physical_path)),
                physical_path) >= 0) {
        candidate.physical_path = physical_path;
      }
      if (candidate.physical_path.empty())
        candidate.physical_path = candidate.path;
      candidates.push_back(candidate);
    }
    closedir(directory);

    std::sort(candidates.begin(), candidates.end(),
              [](const MenuGamepadDevice &left,
                 const MenuGamepadDevice &right) {
                if (left.physical_path != right.physical_path)
                  return left.physical_path < right.physical_path;
                return left.path < right.path;
              });
    while (candidates.size() > kMaximumMenuGamepads) {
      close(candidates.back().fd);
      candidates.pop_back();
    }

    bool unchanged = candidates.size() == devices_.size();
    for (size_t i = 0; unchanged && i < candidates.size(); ++i) {
      unchanged = candidates[i].path == devices_[i].path &&
                  candidates[i].physical_path == devices_[i].physical_path &&
                  devices_[i].fd >= 0;
    }
    if (unchanged) {
      for (size_t i = 0; i < candidates.size(); ++i)
        close(candidates[i].fd);
      return true;
    }

    close_devices();
    devices_.swap(candidates);
    for (size_t i = 0; i < devices_.size(); ++i)
      resynchronize(&devices_[i]);
    std::cerr << "deck-menu: " << count()
              << " THEGamepad controller(s) ready for dashboard" << std::endl;
    return true;
  }

  static bool resynchronize(MenuGamepadDevice *gamepad) {
    if (!gamepad || gamepad->fd < 0)
      return false;
    struct input_absinfo x_info;
    struct input_absinfo y_info;
    std::memset(&x_info, 0, sizeof(x_info));
    std::memset(&y_info, 0, sizeof(y_info));
    const size_t key_words = (KEY_MAX + sizeof(unsigned long) * CHAR_BIT) /
                             (sizeof(unsigned long) * CHAR_BIT);
    std::vector<unsigned long> keys(key_words, 0);
    if (ioctl(gamepad->fd, EVIOCGABS(ABS_X), &x_info) != 0 ||
        ioctl(gamepad->fd, EVIOCGABS(ABS_Y), &y_info) != 0 ||
        ioctl(gamepad->fd,
              EVIOCGKEY(keys.size() * sizeof(unsigned long)), &keys[0]) < 0) {
      return false;
    }
    gamepad->x_info = x_info;
    gamepad->y_info = y_info;
    gamepad->x_value = x_info.value;
    gamepad->y_value = y_info.value;
    gamepad->raw_buttons = 0;
    for (unsigned int index = 0; index < 8; ++index) {
      if (bit_is_set(&keys[0], BTN_TRIGGER + index))
        gamepad->raw_buttons |= 1u << index;
    }
    gamepad->state = menu_gamepad_state(*gamepad);
    gamepad->dropping_events = false;
    return true;
  }

  static bool drain_device(MenuGamepadDevice *gamepad,
                           unsigned int *pressed) {
    if (!gamepad || gamepad->fd < 0 || !pressed)
      return false;
    while (true) {
      struct input_event events[32];
      const ssize_t amount = read(gamepad->fd, events, sizeof(events));
      if (amount < 0) {
        if (errno == EINTR)
          continue;
        return errno == EAGAIN || errno == EWOULDBLOCK;
      }
      if (amount == 0 ||
          amount % static_cast<ssize_t>(sizeof(struct input_event)) != 0) {
        return false;
      }
      const size_t count = static_cast<size_t>(amount) / sizeof(events[0]);
      for (size_t i = 0; i < count; ++i) {
        const struct input_event &event = events[i];
        if (gamepad->dropping_events) {
          if (event.type == EV_SYN && event.code == SYN_REPORT) {
            if (!resynchronize(gamepad))
              return false;
          }
          continue;
        }
        if (event.type == EV_SYN && event.code == SYN_DROPPED) {
          gamepad->dropping_events = true;
        } else if (event.type == EV_KEY && event.code >= BTN_TRIGGER &&
                   event.code <= BTN_BASE2) {
          const uint32_t bit = 1u << (event.code - BTN_TRIGGER);
          if (event.value)
            gamepad->raw_buttons |= bit;
          else
            gamepad->raw_buttons &= ~bit;
        } else if (event.type == EV_ABS && event.code == ABS_X) {
          gamepad->x_value = event.value;
        } else if (event.type == EV_ABS && event.code == ABS_Y) {
          gamepad->y_value = event.value;
        } else if (event.type == EV_SYN && event.code == SYN_REPORT) {
          const unsigned int state = menu_gamepad_state(*gamepad);
          *pressed |= state & ~gamepad->state;
          gamepad->state = state;
        }
      }
    }
  }

  void close_devices() {
    for (size_t i = 0; i < devices_.size(); ++i) {
      if (devices_[i].fd >= 0)
        close(devices_[i].fd);
    }
    devices_.clear();
  }

  std::vector<MenuGamepadDevice> devices_;
  int64_t last_scan_ms_;
};

struct MenuKeyboardDevice {
  int fd;
  std::string path;
  std::string name;
  bool left_shift;
  bool right_shift;
  bool dropping_events;
  bool grabbed;

  MenuKeyboardDevice()
      : fd(-1), left_shift(false), right_shift(false),
        dropping_events(false), grabbed(false) {}
};

class MenuKeyboards {
public:
  MenuKeyboards() : last_scan_ms_(0) {}
  ~MenuKeyboards() { close_devices(); }

  size_t count() const {
    size_t connected = 0;
    for (size_t i = 0; i < devices_.size(); ++i)
      connected += devices_[i].fd >= 0 ? 1 : 0;
    return connected;
  }

  bool scan_if_due(bool force, std::string *error) {
    const int64_t now = monotonic_ms();
    if (!force && last_scan_ms_ > 0 && now - last_scan_ms_ < 1000)
      return true;
    last_scan_ms_ = now;
    return scan(error);
  }

  void append_poll_descriptors(std::vector<struct pollfd> *descriptors) const {
    if (!descriptors)
      return;
    for (size_t i = 0; i < devices_.size(); ++i) {
      struct pollfd descriptor;
      descriptor.fd = devices_[i].fd;
      descriptor.events = POLLIN;
      descriptor.revents = 0;
      descriptors->push_back(descriptor);
    }
  }

  unsigned int read_ready(const std::vector<struct pollfd> &descriptors,
                          size_t first_descriptor) {
    unsigned int pressed = 0;
    for (size_t i = 0; i < devices_.size(); ++i) {
      if (first_descriptor + i >= descriptors.size())
        break;
      const short revents = descriptors[first_descriptor + i].revents;
      if (!(revents & (POLLIN | POLLERR | POLLHUP | POLLNVAL)))
        continue;
      if ((revents & POLLIN) && drain_device(&devices_[i], &pressed))
        continue;
      close_device(&devices_[i]);
      last_scan_ms_ = 0;
    }
    return pressed;
  }

  void close_for_child() {
    close_devices();
    last_scan_ms_ = 0;
  }

private:
  bool scan(std::string *error) {
    DIR *directory = opendir("/dev/input");
    if (!directory) {
      if (error)
        *error = errno_message("cannot scan keyboards");
      return false;
    }

    const size_t key_words = (KEY_MAX + sizeof(unsigned long) * CHAR_BIT) /
                             (sizeof(unsigned long) * CHAR_BIT);
    std::vector<MenuKeyboardDevice> candidates;
    for (struct dirent *entry = readdir(directory); entry;
         entry = readdir(directory)) {
      const std::string filename(entry->d_name);
      if (filename.size() <= 5 || filename.compare(0, 5, "event") != 0)
        continue;
      bool numeric = true;
      for (size_t i = 5; i < filename.size(); ++i) {
        numeric = numeric &&
                  std::isdigit(static_cast<unsigned char>(filename[i]));
      }
      if (!numeric)
        continue;

      MenuKeyboardDevice candidate;
      candidate.path = "/dev/input/" + filename;
      candidate.fd = open(candidate.path.c_str(),
                          O_RDONLY | O_NONBLOCK | O_CLOEXEC);
      if (candidate.fd < 0)
        continue;
      std::vector<unsigned long> keys(key_words, 0);
      if (ioctl(candidate.fd,
                EVIOCGBIT(EV_KEY, keys.size() * sizeof(unsigned long)),
                &keys[0]) < 0 ||
          !menu_keyboard_capabilities(&keys[0])) {
        close(candidate.fd);
        continue;
      }
      char name[256] = {};
      if (ioctl(candidate.fd, EVIOCGNAME(sizeof(name)), name) >= 0)
        candidate.name = name;
      if (candidate.name.empty())
        candidate.name = filename;
      candidates.push_back(candidate);
    }
    closedir(directory);

    std::sort(candidates.begin(), candidates.end(),
              [](const MenuKeyboardDevice &left,
                 const MenuKeyboardDevice &right) {
                return left.path < right.path;
              });
    while (candidates.size() > kMaximumMenuKeyboards) {
      close(candidates.back().fd);
      candidates.pop_back();
    }

    bool unchanged = candidates.size() == devices_.size();
    for (size_t i = 0; unchanged && i < candidates.size(); ++i) {
      unchanged = candidates[i].path == devices_[i].path &&
                  devices_[i].fd >= 0;
    }
    if (unchanged) {
      for (size_t i = 0; i < candidates.size(); ++i)
        close(candidates[i].fd);
      return true;
    }

    close_devices();
    devices_.swap(candidates);
    for (size_t i = 0; i < devices_.size(); ++i) {
      if (ioctl(devices_[i].fd, EVIOCGRAB, 1) == 0) {
        devices_[i].grabbed = true;
      } else {
        std::cerr << "deck-menu: warning: cannot exclusively grab "
                  << devices_[i].path << ": " << std::strerror(errno)
                  << std::endl;
      }
      resynchronize(&devices_[i]);
    }
    if (!devices_.empty()) {
      std::cerr << "deck-menu: " << count()
                << " keyboard(s) ready for dashboard" << std::endl;
    }
    return true;
  }

  static bool resynchronize(MenuKeyboardDevice *keyboard) {
    if (!keyboard || keyboard->fd < 0)
      return false;
    const size_t key_words = (KEY_MAX + sizeof(unsigned long) * CHAR_BIT) /
                             (sizeof(unsigned long) * CHAR_BIT);
    std::vector<unsigned long> keys(key_words, 0);
    if (ioctl(keyboard->fd,
              EVIOCGKEY(keys.size() * sizeof(unsigned long)), &keys[0]) < 0) {
      return false;
    }
    keyboard->left_shift = bit_is_set(&keys[0], KEY_LEFTSHIFT);
    keyboard->right_shift = bit_is_set(&keys[0], KEY_RIGHTSHIFT);
    keyboard->dropping_events = false;
    return true;
  }

  static bool drain_device(MenuKeyboardDevice *keyboard,
                           unsigned int *pressed) {
    if (!keyboard || keyboard->fd < 0 || !pressed)
      return false;
    while (true) {
      struct input_event events[32];
      const ssize_t amount = read(keyboard->fd, events, sizeof(events));
      if (amount < 0) {
        if (errno == EINTR)
          continue;
        return errno == EAGAIN || errno == EWOULDBLOCK;
      }
      if (amount == 0 ||
          amount % static_cast<ssize_t>(sizeof(struct input_event)) != 0) {
        return false;
      }
      const size_t count = static_cast<size_t>(amount) / sizeof(events[0]);
      for (size_t i = 0; i < count; ++i) {
        const struct input_event &event = events[i];
        if (keyboard->dropping_events) {
          if (event.type == EV_SYN && event.code == SYN_REPORT &&
              !resynchronize(keyboard)) {
            return false;
          }
          continue;
        }
        if (event.type == EV_SYN && event.code == SYN_DROPPED) {
          keyboard->dropping_events = true;
          continue;
        }
        if (event.type != EV_KEY)
          continue;
        *pressed |= menu_keyboard_event_to_button(
            event.code, event.value, &keyboard->left_shift,
            &keyboard->right_shift);
      }
    }
  }

  static void close_device(MenuKeyboardDevice *keyboard) {
    if (!keyboard || keyboard->fd < 0)
      return;
    if (keyboard->grabbed)
      ioctl(keyboard->fd, EVIOCGRAB, 0);
    close(keyboard->fd);
    keyboard->fd = -1;
    keyboard->grabbed = false;
    keyboard->left_shift = false;
    keyboard->right_shift = false;
    keyboard->dropping_events = false;
  }

  void close_devices() {
    for (size_t i = 0; i < devices_.size(); ++i)
      close_device(&devices_[i]);
    devices_.clear();
  }

  std::vector<MenuKeyboardDevice> devices_;
  int64_t last_scan_ms_;
};

enum MenuGamepadCommand {
  MenuGamepadCommandNone,
  MenuGamepadCommandPrevious,
  MenuGamepadCommandNext,
  MenuGamepadCommandSystemPrevious,
  MenuGamepadCommandSystemNext,
  MenuGamepadCommandConfirm,
  MenuGamepadCommandBack,
  MenuGamepadCommandSettings
};

MenuGamepadCommand menu_gamepad_command(unsigned int pressed, bool wifi_view,
                                        bool settings_view) {
  if ((wifi_view || settings_view) && (pressed & kMenuPadBack))
    return MenuGamepadCommandBack;
  if (wifi_view)
    return MenuGamepadCommandNone;
  if (pressed & kMenuPadSettings)
    return MenuGamepadCommandSettings;
  if (!settings_view && (pressed & kMenuPadSystemPrevious))
    return MenuGamepadCommandSystemPrevious;
  if (!settings_view && (pressed & kMenuPadSystemNext))
    return MenuGamepadCommandSystemNext;
  if (pressed & (kMenuPadLeft | kMenuPadUp))
    return MenuGamepadCommandPrevious;
  if (pressed & (kMenuPadRight | kMenuPadDown))
    return MenuGamepadCommandNext;
  if (pressed & kMenuPadConfirm)
    return MenuGamepadCommandConfirm;
  return MenuGamepadCommandNone;
}

class MenuControllerInputGuard {
public:
  MenuControllerInputGuard()
      : suspended_(false), last_edge_at_(-1) {}

  bool accept_edge(int64_t now) {
    last_edge_at_ = now;
    if (suspended_)
      return false;
    while (!edge_times_.empty() &&
           now - edge_times_.front() >= kMenuControllerBurstWindowMs) {
      edge_times_.erase(edge_times_.begin());
    }
    edge_times_.push_back(now);
    if (edge_times_.size() <= kMenuControllerBurstLimit)
      return true;
    suspended_ = true;
    return false;
  }

  bool recover_if_quiet(int64_t now) {
    if (!suspended_ || last_edge_at_ < 0 ||
        now - last_edge_at_ < kMenuControllerQuietResetMs) {
      return false;
    }
    edge_times_.clear();
    suspended_ = false;
    last_edge_at_ = -1;
    return true;
  }

  bool suspended() const { return suspended_; }

private:
  std::vector<int64_t> edge_times_;
  bool suspended_;
  int64_t last_edge_at_;
};

class TtySnapshot {
public:
  TtySnapshot() : fd_(-1), have_termios_(false), have_keyboard_mode_(false),
                  keyboard_mode_(0) {}
  ~TtySnapshot() {
    if (fd_ >= 0)
      close(fd_);
  }

  void capture() {
    fd_ = open("/dev/tty0", O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (fd_ < 0)
      return;
    have_termios_ = tcgetattr(fd_, &termios_) == 0;
    have_keyboard_mode_ = ioctl(fd_, KDGKBMODE, &keyboard_mode_) == 0;
  }

  void restore() {
    if (fd_ >= 0) {
      if (have_keyboard_mode_)
        ioctl(fd_, KDSKBMODE, keyboard_mode_);
      if (have_termios_)
        tcsetattr(fd_, TCSAFLUSH, &termios_);
    }

    const int console = open("/dev/tty0", O_WRONLY | O_CLOEXEC);
    if (console >= 0) {
      static const char display_state[] = "\033[?25l\033[13]\033[9;0]";
      const bool restored =
          write_all(console, display_state, sizeof(display_state) - 1);
      (void)restored;
      close(console);
    }
  }

private:
  int fd_;
  bool have_termios_;
  bool have_keyboard_mode_;
  int keyboard_mode_;
  struct termios termios_;
};

struct ChildResult {
  bool started;
  bool exited_for_touch;
  int status;
  std::string error;
};

bool reconnect_touch(TouchDevice *touch, int64_t *last_attempt,
                     std::string *last_error) {
  if (!touch || touch->fd() >= 0)
    return true;
  const int64_t now = monotonic_ms();
  if (last_attempt && now - *last_attempt < 1000)
    return false;
  if (last_attempt)
    *last_attempt = now;
  std::string error;
  if (touch->discover(&error))
    return true;
  if (last_error && error != *last_error) {
    std::cerr << "deck-menu: " << error << std::endl;
    *last_error = error;
  }
  return false;
}

void signal_child_group(pid_t child, int signal_number) {
  if (kill(-child, signal_number) != 0 && errno == ESRCH)
    kill(child, signal_number);
}

ChildResult run_managed_child(
    const std::string &executable, const std::vector<std::string> &arguments,
    const std::vector<std::pair<std::string, std::string> > &environment,
    const std::string &label, TouchDevice *touch, Framebuffer *framebuffer) {
  ChildResult result;
  result.started = false;
  result.exited_for_touch = false;
  result.status = 0;

  framebuffer->close_for_child();
  TtySnapshot tty;
  tty.capture();
  std::cerr << "deck-menu: launching " << label << std::endl;

  int exec_status_pipe[2] = {-1, -1};
  if (pipe(exec_status_pipe) != 0 ||
      fcntl(exec_status_pipe[0], F_SETFD, FD_CLOEXEC) != 0 ||
      fcntl(exec_status_pipe[1], F_SETFD, FD_CLOEXEC) != 0) {
    const int saved_errno = errno;
    if (exec_status_pipe[0] >= 0)
      close(exec_status_pipe[0]);
    if (exec_status_pipe[1] >= 0)
      close(exec_status_pipe[1]);
    errno = saved_errno;
    result.error = errno_message("cannot create exec status pipe");
    return result;
  }

  const pid_t child = fork();
  if (child < 0) {
    close(exec_status_pipe[0]);
    close(exec_status_pipe[1]);
    result.error = errno_message("fork failed");
    return result;
  }
  if (child == 0) {
    close(exec_status_pipe[0]);
    if (touch)
      touch->close_for_child();
    struct sigaction default_action;
    std::memset(&default_action, 0, sizeof(default_action));
    default_action.sa_handler = SIG_DFL;
    sigemptyset(&default_action.sa_mask);
    sigaction(SIGTERM, &default_action, NULL);
    sigaction(SIGINT, &default_action, NULL);
    sigaction(SIGHUP, &default_action, NULL);
    sigaction(SIGPIPE, &default_action, NULL);
    if (setpgid(0, 0) != 0) {
      const int exec_error = errno;
      const bool sent =
          write_all(exec_status_pipe[1],
                    reinterpret_cast<const char *>(&exec_error),
                    sizeof(exec_error));
      (void)sent;
      _exit(127);
    }
    for (size_t index = 0; index < environment.size(); ++index) {
      if (setenv(environment[index].first.c_str(),
                 environment[index].second.c_str(), 1) != 0) {
        const int exec_error = errno;
        const bool sent = write_all(
            exec_status_pipe[1], reinterpret_cast<const char *>(&exec_error),
            sizeof(exec_error));
        (void)sent;
        _exit(127);
      }
    }
    std::vector<char *> child_arguments;
    child_arguments.push_back(const_cast<char *>(executable.c_str()));
    for (size_t index = 0; index < arguments.size(); ++index)
      child_arguments.push_back(const_cast<char *>(arguments[index].c_str()));
    child_arguments.push_back(NULL);
    execv(executable.c_str(), &child_arguments[0]);
    const int exec_error = errno;
    const bool sent =
        write_all(exec_status_pipe[1],
                  reinterpret_cast<const char *>(&exec_error),
                  sizeof(exec_error));
    (void)sent;
    dprintf(STDERR_FILENO, "deck-menu: cannot exec managed child: %s\n",
            std::strerror(exec_error));
    _exit(127);
  }

  // Close the race in which the parent needs to signal the new process group
  // before the child reaches setpgid(). EACCES merely means exec won the race.
  if (setpgid(child, child) != 0 && errno != EACCES && errno != ESRCH)
    std::cerr << "deck-menu: warning: cannot establish child process group: "
              << std::strerror(errno) << std::endl;

  close(exec_status_pipe[1]);
  int exec_error = 0;
  ssize_t exec_status_size;
  do {
    exec_status_size =
        read(exec_status_pipe[0], &exec_error, sizeof(exec_error));
  } while (exec_status_size < 0 && errno == EINTR);
  close(exec_status_pipe[0]);
  if (exec_status_size == 0) {
    result.started = true;
  } else if (exec_status_size == static_cast<ssize_t>(sizeof(exec_error))) {
    result.error = "cannot start " + label + ": " +
                   std::string(std::strerror(exec_error));
  } else if (exec_status_size < 0) {
    result.error = errno_message("cannot read exec status");
  } else {
    result.error = "invalid exec status response";
  }
  bool term_sent = false;
  bool kill_sent = false;
  int64_t term_sent_at = 0;
  bool corner_hold = false;
  int64_t corner_hold_started = 0;
  int64_t reconnect_attempt = 0;
  std::string touch_error;
  const auto update_corner_hold = [&](bool down, int x, int y) {
    const bool inside = down && x >= 0 && x < kExitHoldWidth && y >= 0 &&
                        y < kExitHoldHeight;
    if (!inside) {
      if (corner_hold)
        std::cerr << "deck-menu: return hold cancelled at " << x << ',' << y
                  << std::endl;
      corner_hold = false;
    } else if (!corner_hold) {
      corner_hold = true;
      corner_hold_started = monotonic_ms();
      std::cerr << "deck-menu: return hold started at " << x << ',' << y
                << std::endl;
    }
  };

  while (true) {
    pid_t waited = waitpid(child, &result.status, WNOHANG);
    if (waited == child)
      break;
    if (waited < 0 && errno != EINTR) {
      result.error = errno_message("waitpid failed");
      signal_child_group(child, SIGKILL);
      while (waitpid(child, &result.status, 0) < 0 && errno == EINTR) {
      }
      break;
    }

    const int64_t now = monotonic_ms();
    if (g_shutdown_requested && !term_sent) {
      signal_child_group(child, SIGTERM);
      term_sent = true;
      term_sent_at = now;
    }

    if (!framebuffer->uses_wayland() && touch && touch->fd() < 0)
      reconnect_touch(touch, &reconnect_attempt, &touch_error);

    struct pollfd descriptor;
    descriptor.fd = framebuffer->uses_wayland()
                        ? framebuffer->input_fd()
                        : (touch ? touch->fd() : -1);
    descriptor.events = POLLIN;
    descriptor.revents = 0;
    const int poll_result = poll(descriptor.fd >= 0 ? &descriptor : NULL,
                                 descriptor.fd >= 0 ? 1 : 0, 40);
    std::vector<TouchReport> reports;
    if (poll_result > 0 && (descriptor.revents & (POLLIN | POLLERR | POLLHUP))) {
      std::string error;
      const bool read_ok =
          framebuffer->uses_wayland()
              ? framebuffer->read_wayland_touch(&reports, &error)
              : touch->read_reports(&reports, &error);
      if (!read_ok) {
        std::cerr << "deck-menu: " << error << std::endl;
        if (!framebuffer->uses_wayland())
          touch->close_device();
        corner_hold = false;
      }
    }

    for (size_t i = 0; i < reports.size(); ++i) {
      update_corner_hold(reports[i].down, reports[i].x, reports[i].y);
    }
    if (reports.empty() && !framebuffer->uses_wayland() && touch &&
        touch->fd() >= 0)
      update_corner_hold(touch->down(), touch->x(), touch->y());
    if (corner_hold && !term_sent &&
        monotonic_ms() - corner_hold_started >= kExitHoldMs) {
      std::cerr << "deck-menu: return hold complete; stopping " << label
                << std::endl;
      signal_child_group(child, SIGTERM);
      term_sent = true;
      term_sent_at = monotonic_ms();
      result.exited_for_touch = true;
    }

    if (term_sent && !kill_sent &&
        monotonic_ms() - term_sent_at >= kChildTermGraceMs) {
      signal_child_group(child, SIGKILL);
      kill_sent = true;
    }
  }

  tty.restore();
  if (result.started) {
    if (WIFEXITED(result.status)) {
      std::cerr << "deck-menu: " << label << " exited with status "
                << WEXITSTATUS(result.status) << std::endl;
    } else if (WIFSIGNALED(result.status)) {
      std::cerr << "deck-menu: " << label << " stopped by signal "
                << WTERMSIG(result.status) << std::endl;
    } else {
      std::cerr << "deck-menu: " << label
                << " stopped with wait status " << result.status << std::endl;
    }
  }
  return result;
}

ChildResult run_game(const std::string &emulator, const GameEntry &game,
                     unsigned int volume, TouchDevice *touch,
                     Framebuffer *framebuffer,
                     const std::string &volume_state = std::string()) {
  ChildResult result;
  result.started = false;
  result.exited_for_touch = false;
  result.status = 0;
  std::string rom_error;
  if (!validate_rom(game.system, game.rom, &rom_error)) {
    result.error = rom_error;
    return result;
  }
  const std::string volume_text = std::to_string(volume);
  std::vector<std::string> arguments;
  if (game.system != "deck")
    arguments.push_back(game.rom);
  std::vector<std::pair<std::string, std::string> > environment;
  environment.push_back(
      std::make_pair("RETRO_DECK_VOLUME_PERCENT", volume_text));
  if (game.system != "deck")
    environment.push_back(std::make_pair("RETRO_DECK_EXIT_HINT", "1"));
  if (framebuffer->uses_wayland())
    environment.push_back(
        std::make_pair("RETRO_DECK_PRESENTATION", "layer-shell"));
  if (!volume_state.empty())
    environment.push_back(
        std::make_pair("RETRO_DECK_VOLUME_STATE", volume_state));
  std::cerr << "deck-menu: game volume " << volume_text << "%" << std::endl;
  TouchDevice *supervisor_touch = touch;
  if (game.system == "deck") {
    if (touch)
      touch->close_device();
    supervisor_touch = NULL;
  }
  return run_managed_child(emulator, arguments, environment, game.id,
                           supervisor_touch, framebuffer);
}

ChildResult run_terminal(const std::string &launcher,
                         const std::string &keymap, const std::string &mode,
                         TouchDevice *touch,
                         Framebuffer *framebuffer) {
  std::vector<std::pair<std::string, std::string> > environment;
  environment.push_back(std::make_pair("RETRO_DECK_KEYMAP", keymap));
  std::vector<std::string> arguments(1, mode);
  return run_managed_child(
      launcher, arguments, environment,
      mode == "shell" ? "terminal" : mode + " REPL", touch, framebuffer);
}

ChildResult run_reboot(const std::string &executable, TouchDevice *touch,
                       Framebuffer *framebuffer) {
  return run_managed_child(executable, std::vector<std::string>(),
                           std::vector<std::pair<std::string, std::string> >(),
                           "reboot", touch, framebuffer);
}

int target_at(const MenuLayout &layout, int x, int y) {
  if (layout.settings_button.contains(x, y))
    return MenuTargetSettings;
  if (layout.game_previous_button.contains(x, y))
    return MenuTargetGamePrevious;
  if (layout.game_next_button.contains(x, y))
    return MenuTargetGameNext;
  for (size_t index = 0; index < layout.system_buttons.size(); ++index) {
    if (layout.system_buttons[index].contains(x, y))
      return MenuTargetSystemBase + static_cast<int>(index);
  }
  for (size_t index = 0; index < layout.game_buttons.size() &&
                         index < layout.visible_game_indices.size();
       ++index) {
    if (layout.game_buttons[index].contains(x, y) &&
        layout.visible_game_indices[index] < static_cast<size_t>(INT_MAX))
      return static_cast<int>(layout.visible_game_indices[index]);
  }
  return MenuTargetNone;
}

int settings_target_at(const SettingsLayout &layout, int x, int y) {
  if (layout.close_button.contains(x, y))
    return SettingsTargetClose;
  if (layout.volume_down_button.contains(x, y))
    return SettingsTargetVolumeDown;
  if (layout.volume_up_button.contains(x, y))
    return SettingsTargetVolumeUp;
  if (layout.brightness_down_button.contains(x, y))
    return SettingsTargetBrightnessDown;
  if (layout.brightness_up_button.contains(x, y))
    return SettingsTargetBrightnessUp;
  if (layout.terminal_button.contains(x, y))
    return SettingsTargetTerminal;
  if (layout.keymap_button.contains(x, y))
    return SettingsTargetKeymap;
  if (layout.wifi_button.contains(x, y))
    return SettingsTargetWifi;
  return SettingsTargetNone;
}

std::string adjacent_system(const std::vector<std::string> &systems,
                            const std::string &active_system, int direction) {
  if (systems.empty())
    return active_system;
  size_t position = 0;
  while (position < systems.size() && systems[position] != active_system)
    ++position;
  if (position == systems.size())
    position = 0;
  if (direction < 0)
    position = position == 0 ? systems.size() - 1 : position - 1;
  else
    position = (position + 1) % systems.size();
  return systems[position];
}

unsigned int volume_after_menu_target(int target, unsigned int volume,
                                      unsigned int last_audible_volume) {
  const unsigned int restore_volume =
      last_audible_volume == 0
          ? kVolumeStep
          : std::min(100U, last_audible_volume);
  if (target == SettingsTargetVolumeUp)
    return volume == 0 ? restore_volume
                       : std::min(100U, volume + kVolumeStep);
  if (target == SettingsTargetVolumeDown)
    return volume > kVolumeStep ? volume - kVolumeStep : 0;
  return volume;
}

unsigned int brightness_after_settings_target(int target,
                                               unsigned int brightness) {
  if (target == SettingsTargetBrightnessUp)
    return std::min(100U, brightness + kBrightnessStep);
  if (target == SettingsTargetBrightnessDown)
    return brightness > kMinimumBrightness
               ? std::max(kMinimumBrightness, brightness - kBrightnessStep)
               : kMinimumBrightness;
  return brightness;
}

enum WifiTarget {
  WifiTargetNone = -1,
  WifiTargetBack = 100,
  WifiTargetSsid,
  WifiTargetPassphrase,
  WifiTargetSave,
  WifiTargetMode,
  WifiTargetShift,
  WifiTargetSpace,
  WifiTargetDelete,
  WifiTargetKeyBase = 200
};

int wifi_target_at(const WifiLayout &layout, int x, int y) {
  if (layout.back_button.contains(x, y))
    return WifiTargetBack;
  if (layout.ssid_field.contains(x, y))
    return WifiTargetSsid;
  if (layout.passphrase_field.contains(x, y))
    return WifiTargetPassphrase;
  if (layout.save_button.contains(x, y))
    return WifiTargetSave;
  if (layout.mode_button.contains(x, y))
    return WifiTargetMode;
  if (layout.shift_button.contains(x, y))
    return WifiTargetShift;
  if (layout.space_button.contains(x, y))
    return WifiTargetSpace;
  if (layout.delete_button.contains(x, y))
    return WifiTargetDelete;
  for (size_t index = 0; index < layout.keys.size(); ++index) {
    if (layout.keys[index].bounds.contains(x, y))
      return WifiTargetKeyBase + static_cast<int>(index);
  }
  return WifiTargetNone;
}

bool valid_wifi_text(const std::string &value, size_t minimum, size_t maximum) {
  if (value.size() < minimum || value.size() > maximum)
    return false;
  for (size_t index = 0; index < value.size(); ++index) {
    const unsigned char ch = static_cast<unsigned char>(value[index]);
    if (ch < 0x20 || ch > 0x7e)
      return false;
  }
  return true;
}

bool save_wifi_profile(const std::string &helper, const std::string &ssid,
                       const std::string &passphrase, std::string *error) {
  if (!valid_wifi_text(ssid, 1, 32)) {
    if (error)
      *error = "SSID MUST BE 1 TO 32 CHARACTERS";
    return false;
  }
  if (!valid_wifi_text(passphrase, 8, 63)) {
    if (error)
      *error = "PASSWORD MUST BE 8 TO 63 CHARACTERS";
    return false;
  }

  int input_pipe[2] = {-1, -1};
  if (pipe(input_pipe) != 0) {
    if (error)
      *error = errno_message("cannot create wifi helper pipe");
    return false;
  }
  const pid_t child = fork();
  if (child < 0) {
    const int saved_errno = errno;
    close(input_pipe[0]);
    close(input_pipe[1]);
    errno = saved_errno;
    if (error)
      *error = errno_message("cannot start wifi helper");
    return false;
  }
  if (child == 0) {
    close(input_pipe[1]);
    if (dup2(input_pipe[0], STDIN_FILENO) < 0)
      _exit(126);
    close(input_pipe[0]);
    execl(helper.c_str(), helper.c_str(), static_cast<char *>(NULL));
    _exit(127);
  }

  close(input_pipe[0]);
  const std::string request = ssid + "\n" + passphrase + "\n";
  const bool wrote = write_all(input_pipe[1], request.data(), request.size());
  const int write_errno = errno;
  const bool closed = close(input_pipe[1]) == 0;
  int status = 0;
  pid_t waited;
  do {
    waited = waitpid(child, &status, 0);
  } while (waited < 0 && errno == EINTR);
  if (!wrote || !closed) {
    errno = write_errno;
    if (error)
      *error = "WIFI PROFILE WRITE FAILED";
    return false;
  }
  if (waited != child || !WIFEXITED(status) || WEXITSTATUS(status) != 0) {
    if (error)
      *error = "WIFI PROFILE WAS NOT SAVED";
    return false;
  }
  return true;
}

bool apply_wifi_target(int target, const WifiLayout &layout, WifiState *state) {
  if (!state)
    return false;
  if (target == WifiTargetSsid) {
    state->field = WifiSsid;
  } else if (target == WifiTargetPassphrase) {
    state->field = WifiPassphrase;
  } else if (target == WifiTargetMode) {
    state->symbols = !state->symbols;
  } else if (target == WifiTargetShift && !state->symbols) {
    state->uppercase = !state->uppercase;
  } else {
    std::string *field = state->field == WifiSsid ? &state->ssid
                                                  : &state->passphrase;
    const size_t limit = state->field == WifiSsid ? 32 : 63;
    if (target == WifiTargetDelete) {
      if (!field->empty())
        field->erase(field->size() - 1);
    } else if (target == WifiTargetSpace) {
      if (field->size() < limit)
        field->push_back(' ');
    } else if (target >= WifiTargetKeyBase &&
               target - WifiTargetKeyBase <
                   static_cast<int>(layout.keys.size())) {
      if (field->size() < limit)
        field->push_back(
            layout.keys[static_cast<size_t>(target - WifiTargetKeyBase)].value);
    } else {
      return false;
    }
  }
  state->status.clear();
  return true;
}

bool validate_executable(const std::string &path, const std::string &role,
                         std::string *error) {
  if (!is_absolute_path(path)) {
    if (error)
      *error = role + " path must be absolute";
    return false;
  }
  struct stat info;
  if (stat(path.c_str(), &info) != 0) {
    if (error)
      *error = errno_message("cannot stat " + role + " " + path);
    return false;
  }
  if (!S_ISREG(info.st_mode) || access(path.c_str(), X_OK) != 0) {
    if (error)
      *error = role + " is not an executable regular file: " + path;
    return false;
  }
  return true;
}

bool inherited_volume(unsigned int *volume, std::string *error) {
  if (!volume)
    return false;
  *volume = 42;
  const char *text = getenv("RETRO_DECK_VOLUME_PERCENT");
  if (!text)
    return true;
  if (!*text) {
    if (error)
      *error = "RETRO_DECK_VOLUME_PERCENT is empty; expected 0 through 100";
    return false;
  }
  unsigned int value = 0;
  for (const char *cursor = text; *cursor; ++cursor) {
    if (*cursor < '0' || *cursor > '9') {
      if (error)
        *error =
            "RETRO_DECK_VOLUME_PERCENT must be an integer from 0 through 100";
      return false;
    }
    value = value * 10 + static_cast<unsigned int>(*cursor - '0');
    if (value > 100) {
      if (error)
        *error =
            "RETRO_DECK_VOLUME_PERCENT must be an integer from 0 through 100";
      return false;
    }
  }
  *volume = value;
  return true;
}

int geometry_test() {
  std::vector<unsigned char> seen(
      static_cast<size_t>(kPhysicalWidth * kPhysicalHeight), 0);
  size_t count = 0;
  for (int logical_y = 0; logical_y < kLogicalHeight; ++logical_y) {
    for (int logical_x = 0; logical_x < kLogicalWidth; ++logical_x) {
      const int physical_column = logical_y;
      const int physical_row = kPhysicalHeight - 1 - logical_x;
      if (physical_column < 0 || physical_column >= kPhysicalWidth ||
          physical_row < 0 || physical_row >= kPhysicalHeight) {
        std::cerr << "geometry-test: mapped pixel is out of bounds\n";
        return 1;
      }
      const size_t index =
          static_cast<size_t>(physical_row) * kPhysicalWidth + physical_column;
      if (seen[index]) {
        std::cerr << "geometry-test: mapping is not one-to-one\n";
        return 1;
      }
      seen[index] = 1;
      ++count;
    }
  }
  if (count != static_cast<size_t>(kLogicalWidth * kLogicalHeight)) {
    std::cerr << "geometry-test: pixel count mismatch\n";
    return 1;
  }
  if (!seen[static_cast<size_t>(1279) * kPhysicalWidth] ||
      !seen[0 * kPhysicalWidth + 479] ||
      seen[0 * kPhysicalWidth + 480]) {
    std::cerr << "geometry-test: corner or unused-region check failed\n";
    return 1;
  }

  Canvas canvas(static_cast<size_t>(kLogicalWidth * kLogicalHeight), 0);
  canvas[0] = 0x1234;
  canvas[canvas.size() - 1] = 0xabcd;
  std::vector<uint16_t> frame(
      static_cast<size_t>(kPhysicalWidth * kPhysicalHeight), 0xdead);
  if (!stage_canvas_for_scanout(canvas, kPhysicalWidth, &frame) ||
      frame[static_cast<size_t>(kPhysicalHeight - 1) * kPhysicalWidth] !=
          0x1234 ||
      frame[kLogicalHeight - 1] != 0xabcd ||
      frame[kLogicalHeight] != 0xdead) {
    std::cerr << "geometry-test: staged scanout transform failed\n";
    return 1;
  }
  std::cout << "geometry-test: OK logical=1280x480 physical=600x1280 "
               "active-columns=0..479\n";
  return 0;
}

struct Options {
  std::string nes_emulator;
  std::string gb_emulator;
  std::string zx_emulator;
  std::string chip8_emulator;
  std::string deck_game;
  std::string chiptune_player;
  std::string chiptune_directory;
  std::string manifest;
  std::string palette;
  std::string settings_icon_directory;
  std::string cover_directory;
  std::string volume_state;
  std::string brightness;
  std::string brightness_max;
  std::string brightness_state;
  std::string keymap_state;
  std::string terminal;
  std::string wifi_helper;
  std::string wifi_status;
  std::string validate_manifest;
  std::string validate_palette;
  bool geometry_test;
  bool help;

  Options() : geometry_test(false), help(false) {}
};

std::string emulator_for_game(const Options &options, const GameEntry &game) {
  if (game.system == "nes")
    return options.nes_emulator;
  if (game.system == "gb" || game.system == "gbc")
    return options.gb_emulator;
  if (game.system == "zx")
    return options.zx_emulator;
  if (game.system == "deck")
    return is_built_in_chiptune(game) ? options.chiptune_player
                                      : options.deck_game;
  return options.chip8_emulator;
}

void print_usage(const char *program) {
  std::cerr << "Usage:\n  " << program
            << " --nes-emulator PATH --gb-emulator PATH "
               "--zx-emulator PATH --chip8-emulator PATH "
               "--deck-game PATH --chiptune-player PATH "
               "--chiptune-directory PATH --manifest PATH "
               "--palette PATH "
               "--settings-icon-directory PATH "
               "--cover-directory PATH "
               "--volume-state PATH "
               "--brightness PATH --brightness-max PATH "
               "--brightness-state PATH "
               "--keymap-state PATH --terminal PATH --wifi-helper PATH "
               "--wifi-status PATH\n  "
            << program << " --geometry-test\n";
  std::cerr << "  " << program << " --validate-manifest PATH\n";
  std::cerr << "  " << program << " --validate-palette PATH\n";
}

bool parse_options(int argc, char **argv, Options *options,
                   std::string *error) {
  if (!options)
    return false;
  for (int i = 1; i < argc; ++i) {
    const std::string argument(argv[i]);
    if (argument == "--geometry-test") {
      options->geometry_test = true;
    } else if (argument == "--validate-manifest") {
      if (++i >= argc) {
        if (error)
          *error = "missing value for --validate-manifest";
        return false;
      }
      if (!options->validate_manifest.empty()) {
        if (error)
          *error = "duplicate option --validate-manifest";
        return false;
      }
      options->validate_manifest = argv[i];
    } else if (argument == "--validate-palette") {
      if (++i >= argc) {
        if (error)
          *error = "missing value for --validate-palette";
        return false;
      }
      if (!options->validate_palette.empty()) {
        if (error)
          *error = "duplicate option --validate-palette";
        return false;
      }
      options->validate_palette = argv[i];
    } else if (argument == "--help" || argument == "-h") {
      options->help = true;
    } else if (argument == "--nes-emulator" ||
               argument == "--gb-emulator" ||
               argument == "--zx-emulator" ||
               argument == "--chip8-emulator" ||
               argument == "--deck-game" ||
               argument == "--chiptune-player" ||
               argument == "--chiptune-directory" ||
               argument == "--manifest" ||
               argument == "--palette" ||
               argument == "--settings-icon-directory" ||
               argument == "--cover-directory" ||
               argument == "--volume-state" ||
               argument == "--brightness" ||
               argument == "--brightness-max" ||
               argument == "--brightness-state" ||
               argument == "--keymap-state" || argument == "--terminal" ||
               argument == "--wifi-helper" || argument == "--wifi-status") {
      if (++i >= argc) {
        if (error)
          *error = "missing value for " + argument;
        return false;
      }
      std::string *destination = NULL;
      if (argument == "--nes-emulator")
        destination = &options->nes_emulator;
      else if (argument == "--gb-emulator")
        destination = &options->gb_emulator;
      else if (argument == "--zx-emulator")
        destination = &options->zx_emulator;
      else if (argument == "--chip8-emulator")
        destination = &options->chip8_emulator;
      else if (argument == "--deck-game")
        destination = &options->deck_game;
      else if (argument == "--chiptune-player")
        destination = &options->chiptune_player;
      else if (argument == "--chiptune-directory")
        destination = &options->chiptune_directory;
      else if (argument == "--manifest")
        destination = &options->manifest;
      else if (argument == "--palette")
        destination = &options->palette;
      else if (argument == "--settings-icon-directory")
        destination = &options->settings_icon_directory;
      else if (argument == "--cover-directory")
        destination = &options->cover_directory;
      else if (argument == "--volume-state")
        destination = &options->volume_state;
      else if (argument == "--brightness")
        destination = &options->brightness;
      else if (argument == "--brightness-max")
        destination = &options->brightness_max;
      else if (argument == "--brightness-state")
        destination = &options->brightness_state;
      else if (argument == "--keymap-state")
        destination = &options->keymap_state;
      else if (argument == "--terminal")
        destination = &options->terminal;
      else if (argument == "--wifi-helper")
        destination = &options->wifi_helper;
      else
        destination = &options->wifi_status;
      if (!destination->empty()) {
        if (error)
          *error = "duplicate option " + argument;
        return false;
      }
      *destination = argv[i];
    } else {
      if (error)
        *error = "unknown option " + argument;
      return false;
    }
  }

  if (options->help)
    return true;
  if (options->geometry_test) {
    if (argc != 2) {
      if (error)
        *error = "--geometry-test must be used alone";
      return false;
    }
    return true;
  }
  if (!options->validate_manifest.empty()) {
    if (argc != 3) {
      if (error)
        *error = "--validate-manifest must be used alone";
      return false;
    }
    return true;
  }
  if (!options->validate_palette.empty()) {
    if (argc != 3) {
      if (error)
        *error = "--validate-palette must be used alone";
      return false;
    }
    return true;
  }
  if (options->nes_emulator.empty() || options->gb_emulator.empty() ||
      options->zx_emulator.empty() || options->chip8_emulator.empty() ||
      options->deck_game.empty() ||
      options->chiptune_player.empty() || options->chiptune_directory.empty() ||
      options->manifest.empty() || options->palette.empty() ||
      options->settings_icon_directory.empty() ||
      options->cover_directory.empty() ||
      options->volume_state.empty() || options->brightness.empty() ||
      options->brightness_max.empty() || options->brightness_state.empty() ||
      options->keymap_state.empty() ||
      options->terminal.empty() || options->wifi_helper.empty() ||
      options->wifi_status.empty()) {
    if (error)
      *error = "--nes-emulator, --gb-emulator, --zx-emulator, "
               "--chip8-emulator, --deck-game, --chiptune-player, "
               "--chiptune-directory, --manifest, "
               "--palette, "
               "--settings-icon-directory, "
               "--cover-directory, --volume-state, --brightness, "
               "--brightness-max, --brightness-state, "
               "--keymap-state, --terminal, --wifi-helper, and "
               "--wifi-status are required";
    return false;
  }
  return true;
}

int application_main(const Options &options) {
  std::string error;
  reset_dashboard_palette();
  if (!load_dashboard_palette(options.palette, &error)) {
    std::cerr << "deck-menu: " << error
              << "; using built-in dashboard palette" << std::endl;
    reset_dashboard_palette();
    error.clear();
  }
  if (!is_absolute_path(options.settings_icon_directory)) {
    std::cerr << "deck-menu: settings icon directory must be an absolute path"
              << std::endl;
    return 1;
  }
  if (!load_selected_settings_icon(options.settings_icon_directory, &error)) {
    std::cerr << "deck-menu: " << error
              << "; using the built-in settings icon" << std::endl;
    gSettingsIcon = kDefaultSettingsIcon;
    gSettingsIconImage.clear();
    error.clear();
  }
  if (!validate_executable(options.nes_emulator, "NES emulator", &error) ||
      !validate_executable(options.gb_emulator, "GB/GBC emulator", &error) ||
      !validate_executable(options.zx_emulator, "ZX Spectrum emulator", &error) ||
      !validate_executable(options.chip8_emulator, "CHIP-8 emulator", &error) ||
      !validate_executable(options.deck_game, "Deck game", &error) ||
      !validate_executable(options.chiptune_player, "chiptune player", &error) ||
      !validate_executable(options.terminal, "terminal launcher", &error) ||
      !validate_executable(kRebootExecutable, "reboot command", &error) ||
      !validate_executable(options.wifi_helper, "wifi helper", &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  unsigned int default_volume = 42;
  if (!inherited_volume(&default_volume, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  std::vector<GameEntry> games;
  if (!load_manifest(options.manifest, &games, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }
  if (!is_absolute_path(options.chiptune_directory)) {
    std::cerr << "deck-menu: chiptune directory must be an absolute path"
              << std::endl;
    return 1;
  }
  games.push_back(built_in_lua_entry(options.terminal));
  games.push_back(built_in_lisp_entry(options.terminal));
  games.push_back(built_in_python_entry(options.terminal));
  games.push_back(built_in_scheme_entry(options.terminal));
  games.push_back(built_in_chiptune_entry(options.chiptune_directory));
  games.push_back(built_in_terminal_entry(options.terminal));
  games.push_back(built_in_reboot_entry(kRebootExecutable));
  if (!is_absolute_path(options.cover_directory)) {
    std::cerr << "deck-menu: cover directory must be an absolute path"
              << std::endl;
    return 1;
  }
  std::cerr << "deck-menu: loaded "
            << load_game_covers(options.cover_directory, &games)
            << " local covers" << std::endl;

  unsigned int volume = default_volume;
  if (!load_volume_state(options.volume_state, default_volume, &volume,
                         &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }
  unsigned int last_audible_volume =
      volume != 0 ? volume
                  : (default_volume != 0 ? default_volume : kVolumeStep);

  unsigned int brightness_maximum = 0;
  unsigned int brightness = 0;
  if (!load_brightness(options.brightness, options.brightness_max,
                       options.brightness_state, &brightness_maximum,
                       &brightness, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  std::string keymap;
  if (!load_keymap_state(options.keymap_state, &keymap, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  Framebuffer framebuffer;
  if (!framebuffer.open_device(&error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  TouchDevice touch;
  if (!framebuffer.uses_wayland() && !touch.discover(&error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  MenuGamepads menu_gamepads;
  MenuKeyboards menu_keyboards;
  MenuSoundPlayer menu_sound_player;
  MenuControllerInputGuard controller_input_guard;
  std::string gamepad_error;
  if (!menu_gamepads.scan_if_due(true, &gamepad_error)) {
    std::cerr << "deck-menu: controller navigation unavailable: "
              << gamepad_error << std::endl;
  }
  std::string keyboard_error;
  if (!menu_keyboards.scan_if_due(true, &keyboard_error)) {
    std::cerr << "deck-menu: keyboard navigation unavailable: "
              << keyboard_error << std::endl;
  }

  Canvas canvas;
  MenuLayout layout;
  SettingsLayout settings_layout;
  WifiLayout wifi_layout;
  WifiState wifi_state;
  NetworkStatus network_status = read_network_status(options.wifi_status);
  bool wifi_view = false;
  bool settings_view = false;
  int settings_selection = SettingsTargetVolumeDown;
  size_t game_position = 0;
  std::string active_system = initial_system(games);
  std::string status;
  const auto render_current_screen = [&]() {
    if (wifi_view) {
      render_wifi(wifi_state, network_status, &canvas, &wifi_layout);
    } else if (settings_view) {
      render_settings(volume, brightness, keymap, settings_selection, status,
                      network_status, &canvas, &settings_layout);
    } else {
      render_menu(games, active_system, game_position, status, &canvas,
                  &layout);
    }
  };
  render_current_screen();
  if (!framebuffer.present(canvas, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  int pressed_target = MenuTargetNone;
  int64_t reconnect_attempt = 0;
  int64_t network_refreshed_at = monotonic_ms();
  int64_t reboot_armed_until = 0;
  std::string last_touch_error;
  std::string last_gamepad_error = gamepad_error;
  std::string last_keyboard_error = keyboard_error;

  while (!g_shutdown_requested) {
    menu_sound_player.reap_finished();
    if (controller_input_guard.recover_if_quiet(monotonic_ms())) {
      std::cerr << "deck-menu: controller input resumed after quiet period"
                << std::endl;
    }
    if (!framebuffer.uses_wayland() && touch.fd() < 0) {
      if (reconnect_touch(&touch, &reconnect_attempt, &last_touch_error)) {
        if (wifi_view)
          wifi_state.status = "TOUCHSCREEN RECONNECTED";
        else
          status = "TOUCHSCREEN RECONNECTED";
        render_current_screen();
        framebuffer.present(canvas, NULL);
      }
    }

    gamepad_error.clear();
    if (!menu_gamepads.scan_if_due(false, &gamepad_error)) {
      if (gamepad_error != last_gamepad_error) {
        std::cerr << "deck-menu: controller navigation unavailable: "
                  << gamepad_error << std::endl;
        last_gamepad_error = gamepad_error;
      }
    } else {
      last_gamepad_error.clear();
    }

    keyboard_error.clear();
    if (!menu_keyboards.scan_if_due(false, &keyboard_error)) {
      if (keyboard_error != last_keyboard_error) {
        std::cerr << "deck-menu: keyboard navigation unavailable: "
                  << keyboard_error << std::endl;
        last_keyboard_error = keyboard_error;
      }
    } else {
      last_keyboard_error.clear();
    }

    std::vector<struct pollfd> descriptors;
    struct pollfd touch_descriptor;
    touch_descriptor.fd = framebuffer.uses_wayland() ? framebuffer.input_fd()
                                                     : touch.fd();
    touch_descriptor.events = POLLIN;
    touch_descriptor.revents = 0;
    descriptors.push_back(touch_descriptor);
    const size_t first_gamepad_descriptor = descriptors.size();
    menu_gamepads.append_poll_descriptors(&descriptors);
    const size_t first_keyboard_descriptor = descriptors.size();
    menu_keyboards.append_poll_descriptors(&descriptors);
    const int poll_result =
        poll(&descriptors[0], static_cast<nfds_t>(descriptors.size()), 250);
    if (poll_result < 0) {
      if (errno == EINTR)
        continue;
      std::cerr << "deck-menu: " << errno_message("poll failed") << std::endl;
      return 1;
    }

    if (reboot_armed_until > 0 &&
        !reboot_confirmation_active(reboot_armed_until, monotonic_ms())) {
      reboot_armed_until = 0;
      if (status == kRebootConfirmationText) {
        status.clear();
        render_current_screen();
        framebuffer.present(canvas, NULL);
      }
    }
    const int64_t now = monotonic_ms();
    if ((wifi_view || settings_view) &&
        now - network_refreshed_at >= 2000) {
      const NetworkStatus refreshed = read_network_status(options.wifi_status);
      network_refreshed_at = now;
      if (refreshed != network_status) {
        network_status = refreshed;
        render_current_screen();
        framebuffer.present(canvas, NULL);
      }
    }
    if (poll_result == 0)
      continue;

    unsigned int controller_pressed =
        menu_gamepads.read_ready(descriptors, first_gamepad_descriptor);
    unsigned int keyboard_pressed =
        menu_keyboards.read_ready(descriptors, first_keyboard_descriptor);
    const bool touch_ready =
        descriptors[0].revents & (POLLIN | POLLERR | POLLHUP | POLLNVAL);
    if (!touch_ready && controller_pressed == 0 && keyboard_pressed == 0)
      continue;

    std::vector<TouchReport> reports;
    const bool touch_ok =
        !touch_ready ||
        (framebuffer.uses_wayland()
             ? framebuffer.read_wayland_touch(&reports, &error)
             : touch.read_reports(&reports, &error));
    if (!touch_ok) {
      std::cerr << "deck-menu: " << error << std::endl;
      if (!framebuffer.uses_wayland())
        touch.close_device();
      pressed_target = MenuTargetNone;
      if (wifi_view) {
        wifi_state.status = "WAITING FOR TOUCHSCREEN";
      } else {
        status = "WAITING FOR TOUCHSCREEN";
      }
      render_current_screen();
      framebuffer.present(canvas, NULL);
    }

    const int64_t input_time = monotonic_ms();
    if (controller_pressed != 0) {
      const bool was_suspended = controller_input_guard.suspended();
      if (!controller_input_guard.accept_edge(input_time)) {
        controller_pressed = 0;
        if (!was_suspended) {
          std::cerr << "deck-menu: controller input suspended after burst; "
                       "waiting for quiet"
                    << std::endl;
        }
      }
    }
    const bool sound_active =
        menu_sound_player.quarantines_input(input_time);
    if (menu_sound_blocks_input(sound_active, MenuInputController)) {
      controller_pressed = 0;
    }
    if (menu_sound_blocks_input(sound_active, MenuInputKeyboard)) {
      keyboard_pressed = 0;
    }
    if (menu_sound_blocks_input(sound_active, MenuInputTouch)) {
      reports.clear();
      pressed_target = MenuTargetNone;
    }

    int selected_game = -1;
    std::string terminal_mode;
    bool reboot_requested = false;
    const auto cancel_reboot_confirmation = [&]() {
      reboot_armed_until = 0;
      if (status == kRebootConfirmationText)
        status.clear();
    };
    const auto request_game = [&](int game_index) {
      if (game_index < 0 || game_index >= static_cast<int>(games.size()))
        return;
      const std::string requested_mode =
          terminal_mode_for_game(games[game_index]);
      if (!requested_mode.empty()) {
        terminal_mode = requested_mode;
      } else if (is_built_in_reboot(games[game_index])) {
        const int64_t now = monotonic_ms();
        if (reboot_confirmation_active(reboot_armed_until, now)) {
          reboot_armed_until = 0;
          reboot_requested = true;
          selected_game = game_index;
        } else {
          reboot_armed_until = now + kRebootConfirmMs;
          status = kRebootConfirmationText;
          render_current_screen();
          framebuffer.present(canvas, NULL);
        }
      } else {
        selected_game = game_index;
      }
    };
    const auto play_menu_sound = [&](MenuSoundCue cue) {
      if (volume == 0)
        return;
      std::string sound_error;
      if (!menu_sound_player.play(cue, volume, &sound_error))
        std::cerr << "deck-menu: " << sound_error << std::endl;
    };
    const auto apply_volume_target = [&](int target) {
      const unsigned int requested =
          volume_after_menu_target(target, volume, last_audible_volume);
      std::string state_error;
      if (save_volume_state(options.volume_state, requested, &state_error)) {
        volume = requested;
        if (volume != 0)
          last_audible_volume = volume;
        status = volume == 0
                     ? "GAME VOLUME MUTED"
                     : "GAME VOLUME " + std::to_string(volume) + "%";
        render_current_screen();
        framebuffer.present(canvas, NULL);
        if (volume == 0) {
          menu_sound_player.stop();
        } else {
          std::string sound_error;
          if (!menu_sound_player.play(MenuSoundCueVolume, volume,
                                      &sound_error)) {
            status = "VOLUME SAVED; CONFIRMATION TONE FAILED";
            std::cerr << "deck-menu: " << sound_error << std::endl;
            render_current_screen();
            framebuffer.present(canvas, NULL);
          }
        }
      } else {
        status = "VOLUME STATE ERROR";
        std::cerr << "deck-menu: " << state_error << std::endl;
        render_current_screen();
        framebuffer.present(canvas, NULL);
      }
    };

    const auto apply_brightness_target = [&](int target) {
      const unsigned int requested =
          brightness_after_settings_target(target, brightness);
      std::string state_error;
      if (set_brightness_percent(options.brightness, options.brightness_state,
                                 brightness_maximum, requested,
                                 &state_error)) {
        brightness = requested;
        status = "BRIGHTNESS " + std::to_string(brightness) + "%";
      } else {
        status = "BRIGHTNESS ERROR - CHECK LOG";
        std::cerr << "deck-menu: " << state_error << std::endl;
      }
      render_current_screen();
      framebuffer.present(canvas, NULL);
    };

    const auto toggle_keymap = [&]() {
      const std::string requested = keymap == "cz" ? "us" : "cz";
      std::string state_error;
      if (save_keymap_state(options.keymap_state, requested, &state_error)) {
        keymap = requested;
        status = keymap == "cz" ? "TERMINAL KEYS: CZECH"
                                : "TERMINAL KEYS: US ANSI";
      } else {
        status = "KEYMAP STATE ERROR";
        std::cerr << "deck-menu: " << state_error << std::endl;
      }
      render_current_screen();
      framebuffer.present(canvas, NULL);
    };

    const auto activate_settings_target = [&](int target) {
      settings_selection = target;
      if (target == SettingsTargetClose) {
        settings_view = false;
        status.clear();
        render_current_screen();
        framebuffer.present(canvas, NULL);
        play_menu_sound(MenuSoundCueBack);
      } else if (target == SettingsTargetVolumeDown ||
                 target == SettingsTargetVolumeUp) {
        apply_volume_target(target);
      } else if (target == SettingsTargetBrightnessDown ||
                 target == SettingsTargetBrightnessUp) {
        apply_brightness_target(target);
        play_menu_sound(target == SettingsTargetBrightnessDown
                            ? MenuSoundCuePrevious
                            : MenuSoundCueNext);
      } else if (target == SettingsTargetTerminal) {
        terminal_mode = "shell";
        play_menu_sound(MenuSoundCueConfirm);
      } else if (target == SettingsTargetKeymap) {
        toggle_keymap();
        play_menu_sound(MenuSoundCueConfirm);
      } else if (target == SettingsTargetWifi) {
        wifi_view = true;
        wifi_state.status.clear();
        render_current_screen();
        framebuffer.present(canvas, NULL);
        play_menu_sound(MenuSoundCueConfirm);
      }
    };

    const MenuGamepadCommand controller_command =
        menu_gamepad_command(controller_pressed | keyboard_pressed, wifi_view,
                             settings_view);
    if (controller_command != MenuGamepadCommandNone) {
      pressed_target = MenuTargetNone;
      reports.clear();
    }
    if (controller_command == MenuGamepadCommandBack) {
      cancel_reboot_confirmation();
      if (wifi_view) {
        wifi_view = false;
        status = "WIFI EDITOR CLOSED";
      } else if (settings_view) {
        settings_view = false;
        status.clear();
      }
      render_current_screen();
      framebuffer.present(canvas, NULL);
      play_menu_sound(MenuSoundCueBack);
    } else if (controller_command == MenuGamepadCommandSettings) {
      cancel_reboot_confirmation();
      settings_view = !settings_view;
      if (settings_view)
        settings_selection = SettingsTargetVolumeDown;
      status.clear();
      render_current_screen();
      framebuffer.present(canvas, NULL);
      play_menu_sound(settings_view ? MenuSoundCueConfirm : MenuSoundCueBack);
    } else if (controller_command == MenuGamepadCommandSystemPrevious ||
               controller_command == MenuGamepadCommandSystemNext) {
      cancel_reboot_confirmation();
      active_system = adjacent_system(
          layout.systems, active_system,
          controller_command == MenuGamepadCommandSystemPrevious ? -1 : 1);
      game_position = 0;
      status.clear();
      render_current_screen();
      framebuffer.present(canvas, NULL);
      play_menu_sound(controller_command == MenuGamepadCommandSystemPrevious
                          ? MenuSoundCuePrevious
                          : MenuSoundCueNext);
    } else if (controller_command == MenuGamepadCommandPrevious ||
               controller_command == MenuGamepadCommandNext) {
      cancel_reboot_confirmation();
      bool moved = false;
      if (settings_view) {
        if (controller_command == MenuGamepadCommandPrevious) {
          settings_selection =
              settings_selection <= SettingsTargetVolumeDown
                  ? SettingsTargetWifi
                  : settings_selection - 1;
        } else {
          settings_selection =
              settings_selection >= SettingsTargetWifi
                  ? SettingsTargetVolumeDown
                  : settings_selection + 1;
        }
        moved = true;
      } else if (!layout.game_indices.empty()) {
        if (controller_command == MenuGamepadCommandPrevious)
          game_position = game_position == 0
                              ? layout.game_indices.size() - 1
                              : game_position - 1;
        else
          game_position = (game_position + 1) % layout.game_indices.size();
        moved = true;
      }
      status.clear();
      render_current_screen();
      framebuffer.present(canvas, NULL);
      if (moved) {
        play_menu_sound(controller_command == MenuGamepadCommandPrevious
                            ? MenuSoundCuePrevious
                            : MenuSoundCueNext);
      }
    } else if (controller_command == MenuGamepadCommandConfirm) {
      if (settings_view) {
        activate_settings_target(settings_selection);
      } else if (layout.shown_game_index < games.size()) {
        if (!is_built_in_reboot(games[layout.shown_game_index]))
          cancel_reboot_confirmation();
        request_game(static_cast<int>(layout.shown_game_index));
        play_menu_sound(MenuSoundCueConfirm);
      }
    }

    for (size_t i = 0; i < reports.size(); ++i) {
      const TouchReport &report = reports[i];
      if (report.pressed) {
        pressed_target =
            wifi_view
                ? wifi_target_at(wifi_layout, report.x, report.y)
                : (settings_view
                       ? settings_target_at(settings_layout, report.x, report.y)
                       : target_at(layout, report.x, report.y));
      }
      if (!report.released)
        continue;
      const int released_target =
          wifi_view
              ? wifi_target_at(wifi_layout, report.x, report.y)
              : (settings_view
                     ? settings_target_at(settings_layout, report.x, report.y)
                     : target_at(layout, report.x, report.y));
      const bool released_reboot =
          !wifi_view && !settings_view && released_target >= 0 &&
          released_target < static_cast<int>(games.size()) &&
          is_built_in_reboot(games[released_target]);
      if (reboot_armed_until > 0 && !released_reboot) {
        cancel_reboot_confirmation();
      }

      if (wifi_view && pressed_target == released_target) {
        if (released_target == WifiTargetBack) {
          wifi_view = false;
          status = "WIFI EDITOR CLOSED";
          render_current_screen();
          play_menu_sound(MenuSoundCueBack);
        } else if (released_target == WifiTargetSave) {
          std::string wifi_error;
          if (save_wifi_profile(options.wifi_helper, wifi_state.ssid,
                                wifi_state.passphrase, &wifi_error)) {
            wifi_state.passphrase.clear();
            wifi_state.status =
                "WIFI SAVED - USED AFTER CURRENT WIFI DISCONNECTS";
          } else {
            wifi_state.status = wifi_error;
          }
          render_current_screen();
          play_menu_sound(MenuSoundCueConfirm);
        } else if (apply_wifi_target(released_target, wifi_layout,
                                     &wifi_state)) {
          render_current_screen();
          play_menu_sound(MenuSoundCueNext);
        }
        framebuffer.present(canvas, NULL);
      } else if (settings_view && pressed_target == released_target &&
                 pressed_target != SettingsTargetNone) {
        activate_settings_target(released_target);
      } else if (!wifi_view && !settings_view &&
                 pressed_target == MenuTargetSettings &&
                 released_target == MenuTargetSettings) {
        settings_view = true;
        settings_selection = SettingsTargetVolumeDown;
        status.clear();
        render_current_screen();
        framebuffer.present(canvas, NULL);
        play_menu_sound(MenuSoundCueConfirm);
      } else if (!wifi_view && !settings_view &&
                 pressed_target == released_target &&
                 pressed_target >= MenuTargetSystemBase &&
                 pressed_target - MenuTargetSystemBase <
                     static_cast<int>(layout.systems.size())) {
        const std::string requested_system = layout.systems[static_cast<size_t>(
            pressed_target - MenuTargetSystemBase)];
        const bool moved = requested_system != active_system;
        active_system = requested_system;
        game_position = 0;
        status.clear();
        render_current_screen();
        framebuffer.present(canvas, NULL);
        if (moved)
          play_menu_sound(MenuSoundCueNext);
      } else if (!wifi_view && !settings_view &&
                 pressed_target == released_target &&
                 (pressed_target == MenuTargetGamePrevious ||
                  pressed_target == MenuTargetGameNext) &&
                 !layout.game_indices.empty()) {
        if (pressed_target == MenuTargetGamePrevious) {
          game_position = game_position == 0
                              ? layout.game_indices.size() - 1
                              : game_position - 1;
        } else {
          game_position = (game_position + 1) % layout.game_indices.size();
        }
        status.clear();
        render_current_screen();
        framebuffer.present(canvas, NULL);
        play_menu_sound(pressed_target == MenuTargetGamePrevious
                            ? MenuSoundCuePrevious
                            : MenuSoundCueNext);
      } else if (!wifi_view && !settings_view && pressed_target >= 0 &&
                 pressed_target == released_target &&
                 pressed_target < static_cast<int>(games.size())) {
        request_game(pressed_target);
        play_menu_sound(MenuSoundCueConfirm);
      }
      pressed_target = MenuTargetNone;
    }

    if (selected_game < 0 && terminal_mode.empty() && !reboot_requested)
      continue;

    const std::string terminal_title = terminal_program_title(terminal_mode);
    status = !terminal_mode.empty()
                 ? "STARTING " + terminal_title
                 : (reboot_requested
                        ? "REBOOTING"
                        : "STARTING " + games[selected_game].title);
    render_current_screen();
    framebuffer.present(canvas, NULL);

    // Close dashboard readers so the launched program gets a fresh controller
    // queue and the menu cannot accumulate gameplay events while it waits.
    menu_sound_player.finish();
    menu_gamepads.close_for_child();
    menu_keyboards.close_for_child();
    const ChildResult child =
        !terminal_mode.empty()
            ? run_terminal(options.terminal, keymap, terminal_mode, &touch,
                           &framebuffer)
            : (reboot_requested
                   ? run_reboot(games[selected_game].rom, &touch, &framebuffer)
                   : run_game(emulator_for_game(options, games[selected_game]),
                              games[selected_game], volume, &touch,
                              &framebuffer, options.volume_state));
    pressed_target = MenuTargetNone;
    if (g_shutdown_requested)
      break;

    gamepad_error.clear();
    if (!menu_gamepads.scan_if_due(true, &gamepad_error)) {
      std::cerr << "deck-menu: controller navigation unavailable: "
                << gamepad_error << std::endl;
      last_gamepad_error = gamepad_error;
    }
    keyboard_error.clear();
    if (!menu_keyboards.scan_if_due(true, &keyboard_error)) {
      std::cerr << "deck-menu: keyboard navigation unavailable: "
                << keyboard_error << std::endl;
      last_keyboard_error = keyboard_error;
    }

    if (!framebuffer.open_device(&error)) {
      std::cerr << "deck-menu: " << error << std::endl;
      return 1;
    }
    unsigned int child_volume = volume;
    std::string child_volume_error;
    if (load_volume_state(options.volume_state, default_volume, &child_volume,
                          &child_volume_error)) {
      if (child_volume != volume) {
        volume = child_volume;
        if (volume != 0)
          last_audible_volume = volume;
        std::cerr << "deck-menu: child updated game volume to " << volume
                  << "%" << std::endl;
      }
    } else {
      std::cerr << "deck-menu: cannot reload child volume: "
                << child_volume_error << std::endl;
    }
    if (!child.error.empty()) {
      status = !terminal_mode.empty()
                   ? terminal_title + " ERROR - CHECK LOG"
                   : (reboot_requested ? "REBOOT ERROR - CHECK LOG"
                                       : "GAME ERROR - CHECK LOG");
      std::cerr << "deck-menu: " << child.error << std::endl;
    } else if (!child.started) {
      status = !terminal_mode.empty()
                   ? terminal_title + " DID NOT START"
                   : (reboot_requested ? "REBOOT DID NOT START"
                                       : "GAME DID NOT START");
    } else if (child.exited_for_touch) {
      status = !terminal_mode.empty()
                   ? "RETURNED FROM " + terminal_title
                   : (reboot_requested
                          ? "REBOOT CANCELLED"
                          : "RETURNED FROM " + games[selected_game].title);
    } else if (WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0) {
      status = !terminal_mode.empty()
                   ? terminal_title + " EXITED"
                   : (reboot_requested
                          ? "REBOOT COMMAND EXITED"
                          : games[selected_game].title + " EXITED");
    } else if (WIFEXITED(child.status)) {
      const std::string exit_status =
          std::to_string(WEXITSTATUS(child.status));
      status = !terminal_mode.empty()
                   ? terminal_title + " EXITED (STATUS " + exit_status + ")"
                   : (reboot_requested
                          ? "REBOOT FAILED (STATUS " + exit_status + ")"
                          : games[selected_game].title + " EXITED (STATUS " +
                                exit_status + ")");
    } else if (WIFSIGNALED(child.status)) {
      const std::string signal_status =
          std::to_string(WTERMSIG(child.status));
      status = !terminal_mode.empty()
                   ? terminal_title + " STOPPED (SIGNAL " + signal_status +
                         ")"
                   : (reboot_requested
                          ? "REBOOT STOPPED (SIGNAL " + signal_status + ")"
                          : games[selected_game].title + " STOPPED (SIGNAL " +
                                signal_status + ")");
    } else {
      status = !terminal_mode.empty()
                   ? terminal_title + " STOPPED"
                   : (reboot_requested
                          ? "REBOOT STOPPED"
                          : games[selected_game].title + " STOPPED");
    }
    render_current_screen();
    if (!framebuffer.present(canvas, &error)) {
      std::cerr << "deck-menu: " << error << std::endl;
      return 1;
    }
  }

  framebuffer.close_device();
  return 0;
}

} // namespace

int main(int argc, char **argv) {
  Options options;
  std::string error;
  if (!parse_options(argc, argv, &options, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    print_usage(argv[0]);
    return 2;
  }
  if (options.help) {
    print_usage(argv[0]);
    return 0;
  }
  if (options.geometry_test)
    return geometry_test();
  if (!options.validate_manifest.empty()) {
    std::vector<GameEntry> games;
    if (!load_manifest(options.validate_manifest, &games, &error)) {
      std::cerr << "deck-menu: " << error << std::endl;
      return 1;
    }
    std::cout << "deck-menu: manifest contains " << games.size()
              << " valid games" << std::endl;
    return 0;
  }
  if (!options.validate_palette.empty()) {
    reset_dashboard_palette();
    if (!load_dashboard_palette(options.validate_palette, &error)) {
      std::cerr << "deck-menu: " << error << std::endl;
      return 1;
    }
    std::cout << "deck-menu: palette contains " << kPaletteTokenCount
              << " valid roles" << std::endl;
    return 0;
  }

  struct sigaction action;
  std::memset(&action, 0, sizeof(action));
  action.sa_handler = signal_handler;
  sigemptyset(&action.sa_mask);
  sigaction(SIGTERM, &action, NULL);
  sigaction(SIGINT, &action, NULL);
  sigaction(SIGHUP, &action, NULL);
  signal(SIGPIPE, SIG_IGN);

  try {
    return application_main(options);
  } catch (const std::exception &exception) {
    std::cerr << "deck-menu: fatal error: " << exception.what() << std::endl;
  } catch (...) {
    std::cerr << "deck-menu: unknown fatal error" << std::endl;
  }
  return 1;
}
