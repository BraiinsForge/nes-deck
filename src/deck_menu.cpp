/*
 * deck_menu.cpp - small touch-first launcher for the Braiins Deck
 *
 * Runtime interface:
 *
 *   deck-menu --nes-emulator /absolute/path/to/infones \
 *             --gb-emulator /absolute/path/to/gb-deck \
 *             --chip8-emulator /absolute/path/to/chip8-deck \
 *             --deck-game /absolute/path/to/ten-seconds-deck \
 *             --manifest /absolute/path/to/games.tsv \
 *             --volume-state /absolute/path/to/volume.state \
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
 * This program deliberately uses only Linux fbdev/evdev and C++11/POSIX APIs.
 * The Deck touch controller already reports the intended 1280x480 landscape
 * coordinate space, while the framebuffer is a 600x1280 portrait RGB565
 * surface.  Rendering therefore uses this transform:
 *
 *   framebuffer column = logical y
 *   framebuffer row    = 1279 - logical x
 */

#include <algorithm>
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
#include <fstream>
#include <iostream>
#include <linux/fb.h>
#include <linux/input.h>
#include <linux/kd.h>
#include <linux/soundcard.h>
#include <poll.h>
#include <limits>
#include <set>
#include <sstream>
#include <string>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>
#include <utility>
#include <vector>

namespace {

const int kLogicalWidth = 1280;
const int kLogicalHeight = 480;
const int kPhysicalWidth = 600;
const int kPhysicalHeight = 1280;
const int kMaxGames = 18;
const off_t kMaximumManifestBytes = 65536;
// Touch is not a gameplay input. A hold anywhere is unambiguous and avoids an
// invisible corner target that may be confused with the inset NES image.
const int kExitHoldWidth = kLogicalWidth;
const int kExitHoldHeight = kLogicalHeight;
const int64_t kExitHoldMs = 2000;
const int64_t kChildTermGraceMs = 4000;
const unsigned int kVolumeStep = 5;

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
         system == "chip8" || system == "deck";
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
  } else if (info.st_size < 1 || info.st_size > 65024) {
    if (error)
      *error = "CHIP-8 ROM must contain 1 through 65024 bytes: " + path;
    ok = false;
  }
  close(fd);
  return ok;
}

struct GameEntry {
  std::string id;
  std::string title;
  std::string system;
  std::string rom;
  RgbColor color;
};

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

bool play_sound_confirmation(unsigned int volume_percent, std::string *error) {
  if (volume_percent == 0 || volume_percent > 100) {
    if (error)
      *error = "enabled volume must be between 1 and 100 for the test tone";
    return false;
  }

  const int fd = open("/dev/dsp", O_WRONLY | O_CLOEXEC);
  if (fd < 0) {
    if (error)
      *error = errno_message("cannot open /dev/dsp for sound confirmation");
    return false;
  }

  int fragment = (4 << 16) | 9;
  int format = AFMT_S16_LE;
  int channels = 1;
  int rate = 44100;
  ioctl(fd, SNDCTL_DSP_SETFRAGMENT, &fragment);
  if (ioctl(fd, SNDCTL_DSP_SETFMT, &format) != 0 ||
      format != AFMT_S16_LE || ioctl(fd, SNDCTL_DSP_CHANNELS, &channels) != 0 ||
      channels != 1 || ioctl(fd, SNDCTL_DSP_SPEED, &rate) != 0 || rate <= 0) {
    const int saved_errno = errno;
    close(fd);
    errno = saved_errno;
    if (error)
      *error = errno_message("cannot configure sound confirmation");
    return false;
  }

  const size_t total_samples = static_cast<size_t>(rate) * 6 / 25;
  const size_t midpoint = total_samples / 2;
  const int amplitude =
      std::max(256, static_cast<int>(5000 * volume_percent / 100));
  const size_t ramp_samples = std::max<size_t>(1, static_cast<size_t>(rate) / 200);
  std::vector<int16_t> tone(total_samples, 0);
  for (size_t i = 0; i < total_samples; ++i) {
    const int frequency = i < midpoint ? 660 : 880;
    const size_t period = std::max<size_t>(2, static_cast<size_t>(rate / frequency));
    int sample = (i % period) < period / 2 ? amplitude : -amplitude;
    const size_t remaining = total_samples - i;
    const size_t envelope =
        std::min(ramp_samples, std::min(i + 1, remaining));
    sample = static_cast<int>(sample * static_cast<int64_t>(envelope) /
                              static_cast<int64_t>(ramp_samples));
    tone[i] = static_cast<int16_t>(sample);
  }

  const bool wrote =
      write_all(fd, reinterpret_cast<const char *>(&tone[0]),
                tone.size() * sizeof(tone[0]));
  const int write_errno = errno;
  if (wrote)
    ioctl(fd, SNDCTL_DSP_SYNC, 0);
  const int close_result = close(fd);
  if (!wrote || close_result != 0) {
    errno = wrote ? errno : write_errno;
    if (error)
      *error = errno_message("cannot play sound confirmation");
    return false;
  }
  return true;
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
    ch = static_cast<unsigned char>(ch - 'a' + 'A');
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

uint16_t contrasting_text(const RgbColor &color) {
  const unsigned int luminance =
      299 * color.red + 587 * color.green + 114 * color.blue;
  return luminance >= 145000 ? rgb565(10, 16, 26) : rgb565(255, 255, 255);
}

uint16_t darker(const RgbColor &color) {
  return rgb565(color.red * 55 / 100, color.green * 55 / 100,
                color.blue * 55 / 100);
}

struct MenuLayout {
  Rect volume_down_button;
  Rect volume_display;
  Rect volume_up_button;
  Rect keymap_button;
  Rect wifi_button;
  Rect terminal_button;
  struct SystemTab {
    Rect bounds;
    std::string system;
  };
  std::vector<SystemTab> system_tabs;
  std::vector<Rect> game_buttons;
  std::vector<size_t> game_indices;
};

const int kSystemTargetBase = -100;

bool is_system_target(int target) { return target <= kSystemTargetBase; }

size_t system_target_index(int target) {
  return static_cast<size_t>(kSystemTargetBase - target);
}

struct SystemDefinition {
  const char *system;
  const char *label;
  int width;
};

const SystemDefinition kSystemDefinitions[] = {
    {"nes", "NES", 120},
    {"gb", "GAME BOY", 180},
    {"gbc", "GAME BOY COLOR", 240},
    {"chip8", "CHIP-8", 160},
    {"deck", "DECK", 120},
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

void draw_terminal_icon(Canvas *canvas, const Rect &button, uint16_t color) {
  const Rect screen{button.x + 18, button.y + 10, button.width - 36, 34};
  stroke_rect(canvas, screen, 3, color);
  fill_rect(canvas, Rect{button.x + button.width / 2 - 3, button.y + 44, 6, 7},
            color);
  fill_rect(canvas, Rect{button.x + 24, button.y + 51, button.width - 48, 3},
            color);
  draw_text(canvas, screen.x + 7, screen.y + 9, ">_", 2, color);
}

void render_menu(const std::vector<GameEntry> &games,
                 const std::string &active_system, unsigned int volume,
                 const std::string &keymap,
                 const std::string &status, Canvas *canvas,
                 MenuLayout *layout) {
  if (!canvas || !layout)
    return;
  canvas->assign(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                 rgb565(0, 0, 0));

  draw_text(canvas, 20, 13, "RETRO DECK", 5, rgb565(255, 245, 171));
  draw_text(canvas, 22, 57, "TOUCH A GAME TO PLAY", 2,
            rgb565(180, 180, 180));

  layout->terminal_button = Rect{682, 10, 82, 62};
  fill_rect(canvas, layout->terminal_button, rgb565(25, 25, 25));
  stroke_rect(canvas, layout->terminal_button, 3, rgb565(130, 130, 130));
  draw_terminal_icon(canvas, layout->terminal_button, rgb565(245, 245, 245));

  layout->keymap_button = Rect{774, 10, 102, 62};
  fill_rect(canvas, layout->keymap_button, rgb565(25, 25, 25));
  stroke_rect(canvas, layout->keymap_button, 3, rgb565(130, 130, 130));
  draw_centered_text(canvas, layout->keymap_button,
                     keymap == "cz" ? "KEYS CZ" : "KEYS US", 2,
                     rgb565(245, 245, 245));

  layout->wifi_button = Rect{886, 10, 98, 62};
  fill_rect(canvas, layout->wifi_button, rgb565(25, 25, 25));
  stroke_rect(canvas, layout->wifi_button, 3, rgb565(130, 130, 130));
  draw_centered_text(canvas, layout->wifi_button, "WIFI", 2,
                     rgb565(245, 245, 245));

  layout->volume_down_button = Rect{994, 10, 62, 62};
  layout->volume_display = Rect{1062, 10, 130, 62};
  layout->volume_up_button = Rect{1198, 10, 62, 62};
  const RgbColor volume_color =
      volume == 0 ? RgbColor{210, 61, 69} : RgbColor{31, 180, 96};
  fill_rect(canvas, layout->volume_down_button, rgb565(25, 25, 25));
  stroke_rect(canvas, layout->volume_down_button, 3, rgb565(130, 130, 130));
  draw_centered_text(canvas, layout->volume_down_button, "-", 4,
                     rgb565(245, 245, 245));
  fill_rect(canvas, layout->volume_display, volume_color.pixel());
  stroke_rect(canvas, layout->volume_display, 4, darker(volume_color));
  draw_centered_text(canvas, layout->volume_display,
                     "VOL " + std::to_string(volume) + "%", 2,
                     contrasting_text(volume_color));
  fill_rect(canvas, layout->volume_up_button, rgb565(25, 25, 25));
  stroke_rect(canvas, layout->volume_up_button, 3, rgb565(130, 130, 130));
  draw_centered_text(canvas, layout->volume_up_button, "+", 4,
                     rgb565(245, 245, 245));

  layout->system_tabs.clear();
  int tab_x = 12;
  for (size_t definition = 0;
       definition < sizeof(kSystemDefinitions) / sizeof(kSystemDefinitions[0]);
       ++definition) {
    if (!has_system(games, kSystemDefinitions[definition].system))
      continue;
    const Rect tab{tab_x, 84, kSystemDefinitions[definition].width, 48};
    const bool selected = active_system == kSystemDefinitions[definition].system;
    const RgbColor tab_color =
        selected ? RgbColor{255, 245, 171} : RgbColor{25, 25, 25};
    fill_rect(canvas, tab, tab_color.pixel());
    draw_centered_text(canvas, tab, kSystemDefinitions[definition].label, 2,
                       selected ? contrasting_text(tab_color)
                                : rgb565(220, 220, 220));
    layout->system_tabs.push_back(MenuLayout::SystemTab{
        tab, kSystemDefinitions[definition].system});
    tab_x += tab.width + 8;
  }

  layout->game_indices.clear();
  for (size_t index = 0; index < games.size(); ++index) {
    if (games[index].system == active_system)
      layout->game_indices.push_back(index);
  }

  const int game_count = static_cast<int>(layout->game_indices.size());
  int columns = 3;
  if (game_count > 6 && game_count <= 8)
    columns = 4;
  else if (game_count > 8)
    columns = 6;
  const int rows = (game_count + columns - 1) / columns;
  const int layout_rows = std::max(2, rows);
  const int margin_x = 12;
  const int gap = 12;
  const int grid_top = 144;
  const int grid_bottom = 444;
  const int cell_width =
      (kLogicalWidth - 2 * margin_x - (columns - 1) * gap) / columns;
  const int cell_height =
      (grid_bottom - grid_top - (layout_rows - 1) * gap) / layout_rows;

  layout->game_buttons.clear();
  for (int index = 0; index < game_count; ++index) {
    const size_t game_index = layout->game_indices[index];
    const int column = index % columns;
    const int row = index / columns;
    const Rect cell{margin_x + column * (cell_width + gap),
                    grid_top + row * (cell_height + gap), cell_width,
                    cell_height};
    layout->game_buttons.push_back(cell);

    fill_rect(canvas, cell, games[game_index].color.pixel());
    const uint16_t text_color = contrasting_text(games[game_index].color);

    const int title_scale =
        fit_text_scale(games[game_index].title, cell.width - 28, 6, 2);
    const std::string shown_title =
        fit_text_width(games[game_index].title, cell.width - 28, title_scale);
    draw_centered_text(canvas,
                       Rect{cell.x + 14, cell.y, cell.width - 28, cell.height},
                       shown_title, title_scale, text_color);
  }

  const std::string footer =
      status.empty()
          ? "CONSOLE GAMES: HOLD ANYWHERE FOR 2 SECONDS TO RETURN"
          : status;
  const int footer_scale = fit_text_scale(footer, kLogicalWidth - 24, 2, 1);
  const std::string shown_footer =
      fit_text_width(footer, kLogicalWidth - 24, footer_scale);
  draw_centered_text(canvas, Rect{12, 452, kLogicalWidth - 24, 28},
                     shown_footer, footer_scale, rgb565(190, 190, 190));
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
  const uint16_t background = active ? rgb565(55, 94, 180) : rgb565(29, 29, 29);
  fill_rect(canvas, bounds, background);
  stroke_rect(canvas, bounds, 3,
              active ? rgb565(160, 190, 255) : rgb565(105, 105, 105));
  const int scale = fit_text_scale(label, bounds.width - 12, 3, 1);
  draw_centered_text(canvas, bounds, label, scale, rgb565(250, 250, 250));
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

void render_wifi(const WifiState &state, Canvas *canvas, WifiLayout *layout) {
  if (!canvas || !layout)
    return;
  canvas->assign(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                 rgb565(0, 0, 0));
  layout->keys.clear();
  layout->back_button = Rect{16, 10, 120, 62};
  layout->ssid_field = Rect{330, 10, 310, 62};
  layout->passphrase_field = Rect{650, 10, 330, 62};
  layout->save_button = Rect{990, 10, 274, 62};
  draw_wifi_button(canvas, layout->back_button, "BACK", false);
  draw_text(canvas, 158, 25, "ADD WIFI", 3, rgb565(255, 245, 171));

  const uint16_t field_bg = rgb565(20, 20, 20);
  fill_rect(canvas, layout->ssid_field, field_bg);
  stroke_rect(canvas, layout->ssid_field, 3,
              state.field == WifiSsid ? rgb565(120, 165, 255)
                                      : rgb565(95, 95, 95));
  draw_text(canvas, layout->ssid_field.x + 10, layout->ssid_field.y + 7,
            "SSID", 1, rgb565(175, 175, 175));
  draw_text(canvas, layout->ssid_field.x + 10, layout->ssid_field.y + 28,
            tail_for_field(state.ssid, 19), 2, rgb565(250, 250, 250));

  fill_rect(canvas, layout->passphrase_field, field_bg);
  stroke_rect(canvas, layout->passphrase_field, 3,
              state.field == WifiPassphrase ? rgb565(120, 165, 255)
                                            : rgb565(95, 95, 95));
  draw_text(canvas, layout->passphrase_field.x + 10,
            layout->passphrase_field.y + 7, "PASSWORD", 1,
            rgb565(175, 175, 175));
  draw_text(canvas, layout->passphrase_field.x + 10,
            layout->passphrase_field.y + 28,
            tail_for_field(std::string(state.passphrase.size(), '*'), 20), 2,
            rgb565(250, 250, 250));
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
  draw_wifi_button(canvas, layout->shift_button, "SHIFT",
                   !state.symbols && state.uppercase);
  draw_wifi_button(canvas, layout->space_button, "SPACE", false);
  draw_wifi_button(canvas, layout->delete_button, "DELETE", false);

  const std::string footer = state.status.empty()
                                 ? "SAVING DOES NOT INTERRUPT CURRENT WIFI"
                                 : state.status;
  const int scale = fit_text_scale(footer, kLogicalWidth - 24, 2, 1);
  draw_centered_text(canvas, Rect{12, 442, kLogicalWidth - 24, 30}, footer,
                     scale, rgb565(190, 190, 190));
}

class Framebuffer {
public:
  Framebuffer() : fd_(-1), memory_(NULL), map_size_(0), stride_(0) {}
  ~Framebuffer() { close_device(); }

  bool open_device(std::string *error) {
    close_device();
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
    if (ioctl(fd_, FBIOBLANK, FB_BLANK_UNBLANK) != 0 && errno != EINVAL &&
        errno != ENOTTY) {
      std::cerr << "deck-menu: warning: cannot unblank framebuffer: "
                << std::strerror(errno) << std::endl;
    }
    return true;
  }

  bool present(const Canvas &canvas, std::string *error) {
    if (!memory_ ||
        canvas.size() !=
            static_cast<size_t>(kLogicalWidth * kLogicalHeight)) {
      if (error)
        *error = "framebuffer or logical canvas is not initialized";
      return false;
    }

    std::memset(memory_, 0, map_size_);
    for (int logical_x = 0; logical_x < kLogicalWidth; ++logical_x) {
      const int physical_row = kPhysicalHeight - 1 - logical_x;
      uint16_t *destination = reinterpret_cast<uint16_t *>(
          memory_ + static_cast<size_t>(physical_row) * stride_);
      for (int logical_y = 0; logical_y < kLogicalHeight; ++logical_y) {
        destination[logical_y] =
            canvas[static_cast<size_t>(logical_y) * kLogicalWidth +
                   logical_x];
      }
    }
    return true;
  }

  void close_device() {
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
  }

private:
  int fd_;
  unsigned char *memory_;
  size_t map_size_;
  int stride_;
};

bool bit_is_set(const unsigned long *bits, unsigned int bit) {
  const unsigned int bits_per_word = sizeof(unsigned long) * CHAR_BIT;
  return (bits[bit / bits_per_word] &
          (1UL << (bit % bits_per_word))) != 0;
}

struct TouchReport {
  int x;
  int y;
  bool down;
  bool pressed;
  bool released;
};

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
    if (fd_ < 0)
      return;
    if (have_keyboard_mode_)
      ioctl(fd_, KDSKBMODE, keyboard_mode_);
    if (have_termios_)
      tcsetattr(fd_, TCSAFLUSH, &termios_);
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

  framebuffer->close_device();
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

    if (touch && touch->fd() < 0)
      reconnect_touch(touch, &reconnect_attempt, &touch_error);

    struct pollfd descriptor;
    descriptor.fd = touch ? touch->fd() : -1;
    descriptor.events = POLLIN;
    descriptor.revents = 0;
    const int poll_result = poll(descriptor.fd >= 0 ? &descriptor : NULL,
                                 descriptor.fd >= 0 ? 1 : 0, 40);
    std::vector<TouchReport> reports;
    if (poll_result > 0 && (descriptor.revents & (POLLIN | POLLERR | POLLHUP))) {
      std::string error;
      if (!touch->read_reports(&reports, &error)) {
        std::cerr << "deck-menu: " << error << std::endl;
        touch->close_device();
        corner_hold = false;
      }
    }

    for (size_t i = 0; i < reports.size(); ++i) {
      update_corner_hold(reports[i].down, reports[i].x, reports[i].y);
    }
    if (reports.empty() && touch && touch->fd() >= 0)
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
                     unsigned int volume,
                     TouchDevice *touch, Framebuffer *framebuffer) {
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
      std::make_pair("INFONES_VOLUME_PERCENT", volume_text));
  environment.push_back(
      std::make_pair("RETRO_DECK_VOLUME_PERCENT", volume_text));
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
                         const std::string &keymap, TouchDevice *touch,
                         Framebuffer *framebuffer) {
  std::vector<std::pair<std::string, std::string> > environment;
  environment.push_back(std::make_pair("RETRO_DECK_KEYMAP", keymap));
  return run_managed_child(
      launcher, std::vector<std::string>(),
      environment, "terminal", touch, framebuffer);
}

int target_at(const MenuLayout &layout, int x, int y) {
  if (layout.volume_down_button.contains(x, y))
    return -2;
  if (layout.wifi_button.contains(x, y))
    return -3;
  if (layout.terminal_button.contains(x, y))
    return -4;
  if (layout.volume_up_button.contains(x, y))
    return -5;
  if (layout.keymap_button.contains(x, y))
    return -6;
  for (size_t i = 0; i < layout.system_tabs.size(); ++i) {
    if (layout.system_tabs[i].bounds.contains(x, y))
      return kSystemTargetBase - static_cast<int>(i);
  }
  for (size_t i = 0; i < layout.game_buttons.size(); ++i) {
    if (layout.game_buttons[i].contains(x, y))
      return static_cast<int>(layout.game_indices[i]);
  }
  return -1;
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
  const char *text = getenv("INFONES_VOLUME_PERCENT");
  if (!text)
    return true;
  if (!*text) {
    if (error)
      *error = "INFONES_VOLUME_PERCENT is empty; expected 0 through 100";
    return false;
  }
  unsigned int value = 0;
  for (const char *cursor = text; *cursor; ++cursor) {
    if (*cursor < '0' || *cursor > '9') {
      if (error)
        *error = "INFONES_VOLUME_PERCENT must be an integer from 0 through 100";
      return false;
    }
    value = value * 10 + static_cast<unsigned int>(*cursor - '0');
    if (value > 100) {
      if (error)
        *error = "INFONES_VOLUME_PERCENT must be an integer from 0 through 100";
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
  std::cout << "geometry-test: OK logical=1280x480 physical=600x1280 "
               "active-columns=0..479\n";
  return 0;
}

struct Options {
  std::string nes_emulator;
  std::string gb_emulator;
  std::string chip8_emulator;
  std::string deck_game;
  std::string manifest;
  std::string volume_state;
  std::string keymap_state;
  std::string terminal;
  std::string wifi_helper;
  bool geometry_test;
  bool help;

  Options() : geometry_test(false), help(false) {}
};

std::string emulator_for_game(const Options &options, const GameEntry &game) {
  if (game.system == "nes")
    return options.nes_emulator;
  if (game.system == "gb" || game.system == "gbc")
    return options.gb_emulator;
  if (game.system == "deck")
    return options.deck_game;
  return options.chip8_emulator;
}

void print_usage(const char *program) {
  std::cerr << "Usage:\n  " << program
            << " --nes-emulator PATH --gb-emulator PATH "
               "--chip8-emulator PATH --deck-game PATH --manifest PATH "
               "--volume-state PATH "
               "--keymap-state PATH --terminal PATH --wifi-helper PATH\n  "
            << program << " --geometry-test\n";
}

bool parse_options(int argc, char **argv, Options *options,
                   std::string *error) {
  if (!options)
    return false;
  for (int i = 1; i < argc; ++i) {
    const std::string argument(argv[i]);
    if (argument == "--geometry-test") {
      options->geometry_test = true;
    } else if (argument == "--help" || argument == "-h") {
      options->help = true;
    } else if (argument == "--nes-emulator" ||
               argument == "--gb-emulator" ||
               argument == "--chip8-emulator" ||
               argument == "--deck-game" ||
               argument == "--manifest" ||
               argument == "--volume-state" ||
               argument == "--keymap-state" || argument == "--terminal" ||
               argument == "--wifi-helper") {
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
      else if (argument == "--chip8-emulator")
        destination = &options->chip8_emulator;
      else if (argument == "--deck-game")
        destination = &options->deck_game;
      else if (argument == "--manifest")
        destination = &options->manifest;
      else if (argument == "--volume-state")
        destination = &options->volume_state;
      else if (argument == "--keymap-state")
        destination = &options->keymap_state;
      else if (argument == "--terminal")
        destination = &options->terminal;
      else
        destination = &options->wifi_helper;
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
  if (options->nes_emulator.empty() || options->gb_emulator.empty() ||
      options->chip8_emulator.empty() || options->deck_game.empty() ||
      options->manifest.empty() ||
      options->volume_state.empty() || options->keymap_state.empty() ||
      options->terminal.empty() || options->wifi_helper.empty()) {
    if (error)
      *error = "--nes-emulator, --gb-emulator, --chip8-emulator, --deck-game, "
               "--manifest, --volume-state, --keymap-state, --terminal, and "
               "--wifi-helper are required";
    return false;
  }
  return true;
}

int application_main(const Options &options) {
  std::string error;
  if (!validate_executable(options.nes_emulator, "NES emulator", &error) ||
      !validate_executable(options.gb_emulator, "GB/GBC emulator", &error) ||
      !validate_executable(options.chip8_emulator, "CHIP-8 emulator", &error) ||
      !validate_executable(options.deck_game, "Deck game", &error) ||
      !validate_executable(options.terminal, "terminal launcher", &error) ||
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

  unsigned int volume = default_volume;
  if (!load_volume_state(options.volume_state, default_volume, &volume,
                         &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  std::string keymap;
  if (!load_keymap_state(options.keymap_state, &keymap, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  TouchDevice touch;
  if (!touch.discover(&error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  Framebuffer framebuffer;
  if (!framebuffer.open_device(&error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  Canvas canvas;
  MenuLayout layout;
  WifiLayout wifi_layout;
  WifiState wifi_state;
  bool wifi_view = false;
  std::string active_system = initial_system(games);
  std::string status;
  render_menu(games, active_system, volume, keymap, status, &canvas, &layout);
  if (!framebuffer.present(canvas, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  int pressed_target = -1;
  int64_t reconnect_attempt = 0;
  std::string last_touch_error;

  while (!g_shutdown_requested) {
    if (touch.fd() < 0) {
      if (reconnect_touch(&touch, &reconnect_attempt, &last_touch_error)) {
        if (wifi_view) {
          wifi_state.status = "TOUCHSCREEN RECONNECTED";
          render_wifi(wifi_state, &canvas, &wifi_layout);
        } else {
          status = "TOUCHSCREEN RECONNECTED";
          render_menu(games, active_system, volume, keymap, status, &canvas,
                      &layout);
        }
        framebuffer.present(canvas, NULL);
      }
    }

    struct pollfd descriptor;
    descriptor.fd = touch.fd();
    descriptor.events = POLLIN;
    descriptor.revents = 0;
    const int poll_result = poll(descriptor.fd >= 0 ? &descriptor : NULL,
                                 descriptor.fd >= 0 ? 1 : 0, 250);
    if (poll_result < 0) {
      if (errno == EINTR)
        continue;
      std::cerr << "deck-menu: " << errno_message("poll failed") << std::endl;
      return 1;
    }
    if (poll_result == 0)
      continue;
    if (!(descriptor.revents & (POLLIN | POLLERR | POLLHUP)))
      continue;

    std::vector<TouchReport> reports;
    if (!touch.read_reports(&reports, &error)) {
      std::cerr << "deck-menu: " << error << std::endl;
      touch.close_device();
      pressed_target = -1;
      if (wifi_view) {
        wifi_state.status = "WAITING FOR TOUCHSCREEN";
        render_wifi(wifi_state, &canvas, &wifi_layout);
      } else {
        status = "WAITING FOR TOUCHSCREEN";
        render_menu(games, active_system, volume, keymap, status, &canvas,
                    &layout);
      }
      framebuffer.present(canvas, NULL);
      continue;
    }

    int selected_game = -1;
    bool terminal_requested = false;
    for (size_t i = 0; i < reports.size(); ++i) {
      const TouchReport &report = reports[i];
      if (report.pressed) {
        pressed_target = wifi_view
                             ? wifi_target_at(wifi_layout, report.x, report.y)
                             : target_at(layout, report.x, report.y);
      }
      if (!report.released)
        continue;
      const int released_target =
          wifi_view ? wifi_target_at(wifi_layout, report.x, report.y)
                    : target_at(layout, report.x, report.y);

      if (wifi_view && pressed_target == released_target) {
        if (released_target == WifiTargetBack) {
          wifi_view = false;
          status = "WIFI EDITOR CLOSED";
          render_menu(games, active_system, volume, keymap, status, &canvas,
                      &layout);
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
          render_wifi(wifi_state, &canvas, &wifi_layout);
        } else if (apply_wifi_target(released_target, wifi_layout,
                                     &wifi_state)) {
          render_wifi(wifi_state, &canvas, &wifi_layout);
        }
        framebuffer.present(canvas, NULL);
      } else if (!wifi_view && is_system_target(pressed_target) &&
                 pressed_target == released_target) {
        const size_t tab_index = system_target_index(released_target);
        if (tab_index < layout.system_tabs.size()) {
          active_system = layout.system_tabs[tab_index].system;
          status.clear();
          render_menu(games, active_system, volume, keymap, status, &canvas,
                      &layout);
          framebuffer.present(canvas, NULL);
        }
      } else if (!wifi_view &&
                 (pressed_target == -2 || pressed_target == -5) &&
                 pressed_target == released_target) {
        const unsigned int requested =
            pressed_target == -5
                ? std::min(100U, volume + kVolumeStep)
                : (volume > kVolumeStep ? volume - kVolumeStep : 0U);
        std::string state_error;
        if (save_volume_state(options.volume_state, requested, &state_error)) {
          volume = requested;
          status = volume == 0
                       ? "GAME VOLUME MUTED"
                       : "GAME VOLUME " + std::to_string(volume) + "%";
          if (volume != 0) {
            std::string tone_error;
            if (!play_sound_confirmation(volume, &tone_error)) {
              status = "VOLUME SAVED; CONFIRMATION TONE FAILED";
              std::cerr << "deck-menu: " << tone_error << std::endl;
            }
          }
        } else {
          status = "VOLUME STATE ERROR";
          std::cerr << "deck-menu: " << state_error << std::endl;
        }
        render_menu(games, active_system, volume, keymap, status, &canvas,
                    &layout);
        framebuffer.present(canvas, NULL);
      } else if (!wifi_view && pressed_target == -3 &&
                 released_target == -3) {
        wifi_view = true;
        wifi_state.status.clear();
        render_wifi(wifi_state, &canvas, &wifi_layout);
        framebuffer.present(canvas, NULL);
      } else if (!wifi_view && pressed_target == -4 &&
                 released_target == -4) {
        terminal_requested = true;
      } else if (!wifi_view && pressed_target == -6 &&
                 released_target == -6) {
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
        render_menu(games, active_system, volume, keymap, status, &canvas,
                    &layout);
        framebuffer.present(canvas, NULL);
      } else if (!wifi_view && pressed_target >= 0 &&
                 pressed_target == released_target &&
                 pressed_target < static_cast<int>(games.size())) {
        selected_game = pressed_target;
      }
      pressed_target = -1;
    }

    if (selected_game < 0 && !terminal_requested)
      continue;

    status = terminal_requested ? "STARTING TERMINAL"
                                : "STARTING " + games[selected_game].title;
    render_menu(games, active_system, volume, keymap, status, &canvas, &layout);
    framebuffer.present(canvas, NULL);

    const ChildResult child =
        terminal_requested
            ? run_terminal(options.terminal, keymap, &touch, &framebuffer)
            : run_game(emulator_for_game(options, games[selected_game]),
                       games[selected_game], volume, &touch, &framebuffer);
    pressed_target = -1;
    if (g_shutdown_requested)
      break;

    if (!framebuffer.open_device(&error)) {
      std::cerr << "deck-menu: " << error << std::endl;
      return 1;
    }
    if (!child.error.empty()) {
      status = terminal_requested ? "TERMINAL ERROR - CHECK LOG"
                                  : "GAME ERROR - CHECK LOG";
      std::cerr << "deck-menu: " << child.error << std::endl;
    } else if (!child.started) {
      status = terminal_requested ? "TERMINAL DID NOT START"
                                  : "GAME DID NOT START";
    } else if (child.exited_for_touch) {
      status = terminal_requested ? "RETURNED FROM TERMINAL"
                                  : "RETURNED FROM " +
                                        games[selected_game].title;
    } else if (WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0) {
      status = terminal_requested ? "TERMINAL EXITED"
                                  : games[selected_game].title + " EXITED";
    } else if (WIFEXITED(child.status)) {
      status = terminal_requested
                   ? "TERMINAL EXITED (STATUS " +
                         std::to_string(WEXITSTATUS(child.status)) + ")"
                   : games[selected_game].title + " EXITED (STATUS " +
                         std::to_string(WEXITSTATUS(child.status)) + ")";
    } else if (WIFSIGNALED(child.status)) {
      status = terminal_requested
                   ? "TERMINAL STOPPED (SIGNAL " +
                         std::to_string(WTERMSIG(child.status)) + ")"
                   : games[selected_game].title + " STOPPED (SIGNAL " +
                         std::to_string(WTERMSIG(child.status)) + ")";
    } else {
      status = terminal_requested
                   ? "TERMINAL STOPPED"
                   : games[selected_game].title + " STOPPED";
    }
    render_menu(games, active_system, volume, keymap, status, &canvas, &layout);
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
