#include "menu_catalog.h"

#include "menu_io.h"
#include "menu_text.h"

#include <cctype>
#include <cerrno>
#include <climits>
#include <cstring>
#include <fcntl.h>
#include <fstream>
#include <set>
#include <sys/stat.h>
#include <unistd.h>

const size_t kMaximumCatalogGames = 64;

namespace {

const off_t kMaximumManifestBytes = 65536;

bool read_exact_at(int fd, off_t offset, unsigned char *data, size_t size) {
  size_t used = 0;
  while (used < size) {
    const ssize_t amount =
        pread(fd, data + used, size - used, offset + static_cast<off_t>(used));
    if (amount > 0)
      used += static_cast<size_t>(amount);
    else if (amount < 0 && errno == EINTR)
      continue;
    else
      return false;
  }
  return true;
}

bool valid_id(const std::string &id) {
  if (id.empty() || id.size() > 48)
    return false;
  for (size_t index = 0; index < id.size(); ++index) {
    const unsigned char character = static_cast<unsigned char>(id[index]);
    const bool valid =
        std::islower(character) || std::isdigit(character) || character == '-';
    if (!valid ||
        (index == 0 && !std::islower(character) && !std::isdigit(character))) {
      return false;
    }
    if (character == '-' &&
        (index + 1 == id.size() || (index > 0 && id[index - 1] == '-'))) {
      return false;
    }
  }
  return true;
}

bool is_optional_header(const std::vector<std::string> &fields) {
  return fields.size() == 5 && fields[0] == "id" && fields[1] == "title" &&
         fields[2] == "system" && fields[3] == "rom" &&
         (fields[4] == "color" || fields[4] == "#RRGGBB");
}

} // namespace

RgbColor xterm_color(unsigned int index) {
  static const RgbColor ansi_colors[] = {
      {0, 0, 0},       {128, 0, 0},   {0, 128, 0},   {128, 128, 0},
      {0, 0, 128},     {128, 0, 128}, {0, 128, 128}, {192, 192, 192},
      {128, 128, 128}, {255, 0, 0},   {0, 255, 0},   {255, 255, 0},
      {0, 0, 255},     {255, 0, 255}, {0, 255, 255}, {255, 255, 255},
  };
  static const unsigned int cube_levels[] = {0, 95, 135, 175, 215, 255};

  if (index < 16)
    return ansi_colors[index];
  if (index < 232) {
    const unsigned int cube = index - 16;
    return RgbColor{cube_levels[cube / 36], cube_levels[(cube / 6) % 6],
                    cube_levels[cube % 6]};
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
        candidate.blue == color.blue) {
      return true;
    }
  }
  return false;
}

bool parse_color(const std::string &text, RgbColor *color) {
  if (!color || text.size() != 7 || text[0] != '#')
    return false;
  unsigned int value = 0;
  for (size_t index = 1; index < text.size(); ++index) {
    const char character = text[index];
    unsigned int nibble = 0;
    if (character >= '0' && character <= '9')
      nibble = static_cast<unsigned int>(character - '0');
    else if (character >= 'a' && character <= 'f')
      nibble = static_cast<unsigned int>(character - 'a' + 10);
    else if (character >= 'A' && character <= 'F')
      nibble = static_cast<unsigned int>(character - 'A' + 10);
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
        0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b, 0x03, 0x73, 0x00, 0x83,
        0x00, 0x0c, 0x00, 0x0d, 0x00, 0x08, 0x11, 0x1f, 0x88, 0x89, 0x00, 0x0e,
        0xdc, 0xcc, 0x6e, 0xe6, 0xdd, 0xdd, 0xd9, 0x99, 0xbb, 0xbb, 0x67, 0x63,
        0x6e, 0x0e, 0xec, 0xcc, 0xdd, 0xdc, 0x99, 0x9f, 0xbb, 0xb9, 0x33, 0x3e};
    unsigned char header[0x50] = {};
    if (info.st_size < 0x150 || info.st_size > 8 * 1024 * 1024 ||
        !read_exact_at(fd, 0x100, header, sizeof(header)) ||
        std::memcmp(header + 4, logo, sizeof(logo)) != 0) {
      if (error)
        *error = "ROM has no valid Game Boy header: " + path;
      ok = false;
    } else {
      unsigned char checksum = 0;
      for (size_t index = 0x34; index <= 0x4c; ++index)
        checksum = static_cast<unsigned char>(checksum - header[index] - 1);
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
        *error = "invalid id on manifest line " + std::to_string(line_number);
      return false;
    }
    if (!valid_utf8_text(game.title, 64, false) ||
        trim_ascii_space(game.title) != game.title) {
      if (error)
        *error =
            "invalid title on manifest line " + std::to_string(line_number);
      return false;
    }
    if (!valid_system(game.system)) {
      if (error)
        *error =
            "invalid system on manifest line " + std::to_string(line_number);
      return false;
    }
    if (!valid_utf8_text(game.rom, PATH_MAX - 1, false) ||
        trim_ascii_space(game.rom) != game.rom) {
      if (error)
        *error =
            "invalid ROM path on manifest line " + std::to_string(line_number);
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
        *error =
            "manifest line " + std::to_string(line_number) + ": " + rom_error;
      return false;
    }
    if (!ids.insert(game.id).second) {
      if (error)
        *error = "duplicate id on manifest line " + std::to_string(line_number);
      return false;
    }
    if (!roms.insert(game.rom).second) {
      if (error)
        *error =
            "duplicate ROM on manifest line " + std::to_string(line_number);
      return false;
    }

    games->push_back(game);
    if (games->size() > kMaximumCatalogGames) {
      if (error)
        *error = "manifest has more than " +
                 std::to_string(kMaximumCatalogGames) +
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
