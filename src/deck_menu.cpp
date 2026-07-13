/*
 * deck_menu.cpp - small touch-first launcher for the Braiins Deck
 *
 * Runtime interface:
 *
 *   deck-menu --emulator /absolute/path/to/infones \
 *             --manifest /absolute/path/to/games.tsv \
 *             --sound-state /absolute/path/to/sound.state
 *
 * Manifest rows have exactly six tab-separated fields:
 *
 *   id<TAB>title<TAB>rom<TAB>description<TAB>#RRGGBB<TAB>license
 *
 * Blank lines and lines beginning with '#' are ignored.  An optional header
 * row is accepted.  The sound state contains exactly "on" or "off".
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

bool validate_rom(const std::string &path, std::string *error) {
  if (!is_absolute_path(path)) {
    if (error)
      *error = "ROM path must be absolute: " + path;
    return false;
  }

  const int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
  if (fd < 0) {
    if (error)
      *error = errno_message("cannot open ROM " + path);
    return false;
  }

  struct stat info;
  unsigned char header[4] = {};
  bool ok = true;
  if (fstat(fd, &info) != 0) {
    if (error)
      *error = errno_message("cannot stat ROM " + path);
    ok = false;
  } else if (!S_ISREG(info.st_mode) || info.st_size < 16) {
    if (error)
      *error = "ROM is not a regular iNES file: " + path;
    ok = false;
  } else {
    ssize_t got = 0;
    while (got < static_cast<ssize_t>(sizeof(header))) {
      const ssize_t amount = read(fd, header + got, sizeof(header) - got);
      if (amount > 0) {
        got += amount;
      } else if (amount < 0 && errno == EINTR) {
        continue;
      } else {
        break;
      }
    }
    if (got != static_cast<ssize_t>(sizeof(header)) ||
        std::memcmp(header, "NES\x1a", 4) != 0) {
      if (error)
        *error = "ROM has no iNES header: " + path;
      ok = false;
    }
  }
  close(fd);
  return ok;
}

struct GameEntry {
  std::string id;
  std::string title;
  std::string rom;
  std::string description;
  RgbColor color;
  std::string license;
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
  return fields.size() == 6 && fields[0] == "id" &&
         fields[1] == "title" && fields[2] == "rom" &&
         fields[3] == "description" && fields[5] == "license" &&
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
    if (fields.size() != 6) {
      if (error)
        *error = "manifest line " + std::to_string(line_number) +
                 " must have exactly 6 TSV fields";
      return false;
    }

    GameEntry game;
    game.id = fields[0];
    game.title = fields[1];
    game.rom = fields[2];
    game.description = fields[3];
    game.license = fields[5];

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
    if (!valid_utf8_text(game.description, 160, false)) {
      if (error)
        *error = "invalid description on manifest line " +
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
    if (!valid_utf8_text(game.license, 160, false) ||
        trim_ascii_space(game.license) != game.license) {
      if (error)
        *error = "invalid license on manifest line " +
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
    if (!validate_rom(game.rom, &rom_error)) {
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

bool save_sound_state(const std::string &path, bool sound_on,
                      std::string *error) {
  if (!is_absolute_path(path)) {
    if (error)
      *error = "sound state path must be absolute";
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
        *error = errno_message("cannot create sound state temporary file");
      return false;
    }
  }
  if (fd < 0) {
    if (error)
      *error = "cannot allocate a sound state temporary file";
    return false;
  }

  const char *value = sound_on ? "on\n" : "off\n";
  bool ok = write_all(fd, value, std::strlen(value));
  if (!ok && error)
    *error = errno_message("cannot write sound state");
  if (ok && fsync(fd) != 0) {
    ok = false;
    if (error)
      *error = errno_message("cannot sync sound state");
  }
  if (close(fd) != 0 && ok) {
    ok = false;
    if (error)
      *error = errno_message("cannot close sound state");
  }
  if (ok && rename(temporary.c_str(), path.c_str()) != 0) {
    ok = false;
    if (error)
      *error = errno_message("cannot replace sound state");
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

bool load_sound_state(const std::string &path, bool *sound_on,
                      std::string *error) {
  if (!sound_on || !is_absolute_path(path)) {
    if (error)
      *error = "sound state path must be absolute";
    return false;
  }

  const int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
  if (fd < 0) {
    if (errno != ENOENT) {
      if (error)
        *error = errno_message("cannot open sound state " + path);
      return false;
    }
    *sound_on = true;
    return save_sound_state(path, true, error);
  }

  struct stat state_info;
  if (fstat(fd, &state_info) != 0) {
    const int saved_errno = errno;
    close(fd);
    errno = saved_errno;
    if (error)
      *error = errno_message("cannot stat sound state " + path);
    return false;
  }
  if (!S_ISREG(state_info.st_mode) || state_info.st_size < 0 ||
      state_info.st_size > 64) {
    close(fd);
    if (error)
      *error = "sound state must be a regular file no larger than 64 bytes";
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
      *error = errno_message("cannot read sound state " + path);
    return false;
  }
  if (used == sizeof(buffer)) {
    if (error)
      *error = "sound state is too large";
    return false;
  }
  const std::string value(buffer, used);
  if (value == "on\n") {
    *sound_on = true;
    return true;
  }
  if (value == "off\n") {
    *sound_on = false;
    return true;
  }
  if (error)
    *error = "sound state must contain exactly 'on\\n' or 'off\\n'";
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

std::vector<std::string> wrap_text(const std::string &utf8_text,
                                   size_t max_characters,
                                   size_t max_lines) {
  std::vector<std::string> lines;
  const std::string text = display_ascii(utf8_text);
  std::istringstream words(text);
  std::string word;
  std::string line;
  while (words >> word) {
    if (word.size() > max_characters)
      word = word.substr(0, max_characters > 1 ? max_characters - 1 : 1) +
             (max_characters > 1 ? "-" : "");
    if (line.empty()) {
      line = word;
    } else if (line.size() + 1 + word.size() <= max_characters) {
      line += " " + word;
    } else {
      lines.push_back(line);
      line = word;
      if (lines.size() == max_lines)
        break;
    }
  }
  if (lines.size() < max_lines && !line.empty())
    lines.push_back(line);
  if (lines.size() == max_lines && !lines.empty() &&
      text.size() > lines[0].size() + (max_lines > 1 ? lines[1].size() : 0) +
                        (max_lines > 1 ? 1 : 0)) {
    std::string &last = lines.back();
    if (last.size() >= 3)
      last.replace(last.size() - 3, 3, "...");
  }
  return lines;
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
  Rect sound_button;
  std::vector<Rect> game_buttons;
};

void render_menu(const std::vector<GameEntry> &games, bool sound_on,
                 const std::string &status, Canvas *canvas,
                 MenuLayout *layout) {
  if (!canvas || !layout)
    return;
  canvas->assign(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                 rgb565(7, 11, 28));

  fill_rect(canvas, Rect{0, 0, kLogicalWidth, 84}, rgb565(18, 25, 55));
  fill_rect(canvas, Rect{0, 80, kLogicalWidth, 4}, rgb565(81, 116, 255));
  draw_text(canvas, 20, 14, "NES DECK", 6, rgb565(255, 245, 171));
  draw_text(canvas, 22, 61, "TOUCH A GAME TO PLAY", 2,
            rgb565(178, 196, 255));

  layout->sound_button = Rect{1000, 10, 260, 62};
  const RgbColor sound_color =
      sound_on ? RgbColor{31, 180, 96} : RgbColor{210, 61, 69};
  fill_rect(canvas, layout->sound_button, sound_color.pixel());
  stroke_rect(canvas, layout->sound_button, 4, darker(sound_color));
  draw_centered_text(canvas,
                     Rect{layout->sound_button.x, layout->sound_button.y + 5,
                          layout->sound_button.width, 20},
                     "NEXT GAME", 2, contrasting_text(sound_color));
  draw_centered_text(canvas,
                     Rect{layout->sound_button.x, layout->sound_button.y + 26,
                          layout->sound_button.width, 30},
                     sound_on ? "SOUND ON" : "SOUND OFF", 3,
                     contrasting_text(sound_color));

  const int game_count = static_cast<int>(games.size());
  int columns = 1;
  if (game_count <= 3)
    columns = game_count;
  else if (game_count <= 12)
    columns = 4;
  else
    columns = 6;
  const int rows = (game_count + columns - 1) / columns;
  const int margin_x = 12;
  const int gap = 10;
  const int grid_top = 94;
  const int grid_bottom = 446;
  const int cell_width =
      (kLogicalWidth - 2 * margin_x - (columns - 1) * gap) / columns;
  const int cell_height =
      (grid_bottom - grid_top - (rows - 1) * gap) / rows;

  layout->game_buttons.clear();
  for (int index = 0; index < game_count; ++index) {
    const int column = index % columns;
    const int row = index / columns;
    const Rect cell{margin_x + column * (cell_width + gap),
                    grid_top + row * (cell_height + gap), cell_width,
                    cell_height};
    layout->game_buttons.push_back(cell);

    fill_rect(canvas, cell, games[index].color.pixel());
    stroke_rect(canvas, cell, 4, darker(games[index].color));
    const uint16_t text_color = contrasting_text(games[index].color);

    const int title_scale =
        fit_text_scale(games[index].title, cell.width - 20, 4, 2);
    const std::string shown_title =
        fit_text_width(games[index].title, cell.width - 20, title_scale);
    draw_centered_text(canvas,
                       Rect{cell.x + 8, cell.y + 8, cell.width - 16,
                            7 * title_scale + 6},
                       shown_title, title_scale, text_color);

    if (cell.height >= 100 && !games[index].description.empty()) {
      const size_t max_chars =
          static_cast<size_t>(std::max(8, (cell.width - 22) / 12));
      const std::vector<std::string> lines =
          wrap_text(games[index].description, max_chars, 2);
      const int description_top = cell.y + 18 + 7 * title_scale;
      for (size_t line = 0; line < lines.size(); ++line) {
        draw_centered_text(
            canvas,
            Rect{cell.x + 8, description_top + static_cast<int>(line) * 17,
                 cell.width - 16, 14},
            lines[line], 2, text_color);
      }
    }

    const int license_scale =
        fit_text_scale(games[index].license, cell.width - 20, 2, 1);
    const std::string shown_license =
        fit_text_width(games[index].license, cell.width - 20, license_scale);
    draw_centered_text(canvas,
                       Rect{cell.x + 8, cell.y + cell.height - 22,
                            cell.width - 16, 15},
                       shown_license, license_scale, text_color);
  }

  fill_rect(canvas, Rect{0, 452, kLogicalWidth, 28}, rgb565(12, 17, 39));
  const std::string footer =
      status.empty() ? "IN GAME: HOLD ANYWHERE FOR 2 SECONDS TO RETURN" : status;
  const int footer_scale = fit_text_scale(footer, kLogicalWidth - 24, 2, 1);
  const std::string shown_footer =
      fit_text_width(footer, kLogicalWidth - 24, footer_scale);
  draw_centered_text(canvas, Rect{12, 452, kLogicalWidth - 24, 28},
                     shown_footer, footer_scale, rgb565(205, 216, 255));
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

ChildResult run_game(const std::string &emulator, const GameEntry &game,
                     bool sound_on, unsigned int enabled_volume,
                     TouchDevice *touch, Framebuffer *framebuffer) {
  ChildResult result;
  result.started = false;
  result.exited_for_touch = false;
  result.status = 0;

  std::string rom_error;
  if (!validate_rom(game.rom, &rom_error)) {
    result.error = rom_error;
    return result;
  }

  framebuffer->close_device();
  TtySnapshot tty;
  tty.capture();
  const std::string child_volume =
      sound_on ? std::to_string(enabled_volume) : std::string("0");

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
    if (setenv("INFONES_VOLUME_PERCENT", child_volume.c_str(), 1) != 0) {
      const int exec_error = errno;
      const bool sent =
          write_all(exec_status_pipe[1],
                    reinterpret_cast<const char *>(&exec_error),
                    sizeof(exec_error));
      (void)sent;
      dprintf(STDERR_FILENO, "deck-menu: cannot set sound environment: %s\n",
              std::strerror(exec_error));
      _exit(127);
    }
    execl(emulator.c_str(), emulator.c_str(), game.rom.c_str(),
          static_cast<char *>(NULL));
    const int exec_error = errno;
    const bool sent =
        write_all(exec_status_pipe[1],
                  reinterpret_cast<const char *>(&exec_error),
                  sizeof(exec_error));
    (void)sent;
    dprintf(STDERR_FILENO, "deck-menu: cannot exec %s: %s\n",
            emulator.c_str(), std::strerror(exec_error));
    _exit(127);
  }

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
    result.error = "cannot exec emulator: " +
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
      kill(child, SIGKILL);
      while (waitpid(child, &result.status, 0) < 0 && errno == EINTR) {
      }
      break;
    }

    const int64_t now = monotonic_ms();
    if (g_shutdown_requested && !term_sent) {
      kill(child, SIGTERM);
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
      std::cerr << "deck-menu: return hold complete; stopping game" << std::endl;
      kill(child, SIGTERM);
      term_sent = true;
      term_sent_at = monotonic_ms();
      result.exited_for_touch = true;
    }

    if (term_sent && !kill_sent &&
        monotonic_ms() - term_sent_at >= kChildTermGraceMs) {
      kill(child, SIGKILL);
      kill_sent = true;
    }
  }

  tty.restore();
  return result;
}

int target_at(const MenuLayout &layout, int x, int y) {
  if (layout.sound_button.contains(x, y))
    return -2;
  for (size_t i = 0; i < layout.game_buttons.size(); ++i) {
    if (layout.game_buttons[i].contains(x, y))
      return static_cast<int>(i);
  }
  return -1;
}

bool validate_emulator(const std::string &path, std::string *error) {
  if (!is_absolute_path(path)) {
    if (error)
      *error = "emulator path must be absolute";
    return false;
  }
  struct stat info;
  if (stat(path.c_str(), &info) != 0) {
    if (error)
      *error = errno_message("cannot stat emulator " + path);
    return false;
  }
  if (!S_ISREG(info.st_mode) || access(path.c_str(), X_OK) != 0) {
    if (error)
      *error = "emulator is not an executable regular file: " + path;
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
  std::string emulator;
  std::string manifest;
  std::string sound_state;
  bool geometry_test;
  bool help;

  Options() : geometry_test(false), help(false) {}
};

void print_usage(const char *program) {
  std::cerr << "Usage:\n  " << program
            << " --emulator PATH --manifest PATH --sound-state PATH\n  "
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
    } else if (argument == "--emulator" || argument == "--manifest" ||
               argument == "--sound-state") {
      if (++i >= argc) {
        if (error)
          *error = "missing value for " + argument;
        return false;
      }
      std::string *destination = NULL;
      if (argument == "--emulator")
        destination = &options->emulator;
      else if (argument == "--manifest")
        destination = &options->manifest;
      else
        destination = &options->sound_state;
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
  if (options->emulator.empty() || options->manifest.empty() ||
      options->sound_state.empty()) {
    if (error)
      *error = "--emulator, --manifest, and --sound-state are required";
    return false;
  }
  return true;
}

int application_main(const Options &options) {
  std::string error;
  if (!validate_emulator(options.emulator, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  unsigned int enabled_volume = 42;
  if (!inherited_volume(&enabled_volume, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  std::vector<GameEntry> games;
  if (!load_manifest(options.manifest, &games, &error)) {
    std::cerr << "deck-menu: " << error << std::endl;
    return 1;
  }

  bool sound_on = true;
  if (!load_sound_state(options.sound_state, &sound_on, &error)) {
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
  std::string status;
  render_menu(games, sound_on, status, &canvas, &layout);
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
        status = "TOUCHSCREEN RECONNECTED";
        render_menu(games, sound_on, status, &canvas, &layout);
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
      status = "WAITING FOR TOUCHSCREEN";
      render_menu(games, sound_on, status, &canvas, &layout);
      framebuffer.present(canvas, NULL);
      continue;
    }

    int selected_game = -1;
    for (size_t i = 0; i < reports.size(); ++i) {
      const TouchReport &report = reports[i];
      if (report.pressed)
        pressed_target = target_at(layout, report.x, report.y);
      if (!report.released)
        continue;
      const int released_target = target_at(layout, report.x, report.y);
      if (pressed_target == -2 && released_target == -2) {
        const bool requested = !sound_on;
        std::string state_error;
        if (save_sound_state(options.sound_state, requested, &state_error)) {
          sound_on = requested;
          status = sound_on ? "SOUND ENABLED FOR THE NEXT GAME"
                            : "SOUND MUTED FOR THE NEXT GAME";
          if (sound_on) {
            std::string tone_error;
            if (!play_sound_confirmation(enabled_volume, &tone_error)) {
              status = "SOUND ON; CONFIRMATION TONE FAILED";
              std::cerr << "deck-menu: " << tone_error << std::endl;
            }
          }
        } else {
          status = "SOUND STATE ERROR";
          std::cerr << "deck-menu: " << state_error << std::endl;
        }
        render_menu(games, sound_on, status, &canvas, &layout);
        framebuffer.present(canvas, NULL);
      } else if (pressed_target >= 0 && pressed_target == released_target &&
                 pressed_target < static_cast<int>(games.size())) {
        selected_game = pressed_target;
      }
      pressed_target = -1;
    }

    if (selected_game < 0)
      continue;

    status = "STARTING " + games[selected_game].title;
    render_menu(games, sound_on, status, &canvas, &layout);
    framebuffer.present(canvas, NULL);

    const ChildResult child = run_game(options.emulator, games[selected_game],
                                       sound_on, enabled_volume, &touch,
                                       &framebuffer);
    pressed_target = -1;
    if (g_shutdown_requested)
      break;

    if (!framebuffer.open_device(&error)) {
      std::cerr << "deck-menu: " << error << std::endl;
      return 1;
    }
    if (!child.error.empty()) {
      status = "GAME ERROR - CHECK LOG";
      std::cerr << "deck-menu: " << child.error << std::endl;
    } else if (!child.started) {
      status = "GAME DID NOT START";
    } else if (child.exited_for_touch) {
      status = "RETURNED FROM " + games[selected_game].title;
    } else if (WIFEXITED(child.status) && WEXITSTATUS(child.status) == 0) {
      status = games[selected_game].title + " EXITED";
    } else {
      status = games[selected_game].title + " STOPPED - CHECK LOG";
    }
    render_menu(games, sound_on, status, &canvas, &layout);
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
