#include "chip8_core.h"
#include "deck_runtime.h"

#include <cerrno>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <fstream>
#include <string>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include <vector>

namespace {

const unsigned int PAD_A = 1u << 0;
const unsigned int PAD_B = 1u << 1;
const unsigned int PAD_SELECT = 1u << 2;
const unsigned int PAD_START = 1u << 3;
const unsigned int PAD_UP = 1u << 4;
const unsigned int PAD_DOWN = 1u << 5;
const unsigned int PAD_LEFT = 1u << 6;
const unsigned int PAD_RIGHT = 1u << 7;
const size_t kMaximumConfigBytes = 4096;
const size_t kMaximumRomBytes = 65024;

volatile sig_atomic_t shutdown_requested = 0;

extern "C" int InitJoypadInput(void);
extern "C" unsigned int GetJoypadInput(unsigned int player);

void request_shutdown(int signal_number) {
  (void)signal_number;
  shutdown_requested = 1;
}

enum InputProfile { InputOcto, InputSpaceRacer };

struct Configuration {
  Chip8CoreOptions core;
  InputProfile input;

  Configuration() : input(InputOcto) { Chip8CoreDefaultOptions(&core); }
};

bool parse_boolean(const std::string &text, int *value) {
  if (!value || (text != "0" && text != "1"))
    return false;
  *value = text == "1";
  return true;
}

bool parse_color(const std::string &text, uint32_t *color) {
  if (!color || text.size() != 7 || text[0] != '#')
    return false;
  uint32_t value = 0;
  for (size_t i = 1; i < text.size(); ++i) {
    const char ch = text[i];
    unsigned int digit;
    if (ch >= '0' && ch <= '9')
      digit = static_cast<unsigned int>(ch - '0');
    else if (ch >= 'a' && ch <= 'f')
      digit = static_cast<unsigned int>(ch - 'a' + 10);
    else if (ch >= 'A' && ch <= 'F')
      digit = static_cast<unsigned int>(ch - 'A' + 10);
    else
      return false;
    value = (value << 4) | digit;
  }
  *color = value;
  return true;
}

bool parse_config(const std::string &path, Configuration *configuration,
                  std::string *error) {
  if (!configuration)
    return false;
  struct stat info;
  if (stat(path.c_str(), &info) != 0) {
    if (errno == ENOENT)
      return true;
    if (error)
      *error = "cannot stat " + path + ": " + std::strerror(errno);
    return false;
  }
  if (!S_ISREG(info.st_mode) || info.st_size < 0 ||
      info.st_size > static_cast<off_t>(kMaximumConfigBytes)) {
    if (error)
      *error = "CHIP-8 config must be a regular file no larger than 4096 bytes";
    return false;
  }
  std::ifstream input(path.c_str(), std::ios::in | std::ios::binary);
  std::string line;
  size_t line_number = 0;
  while (std::getline(input, line)) {
    ++line_number;
    if (!line.empty() && line[line.size() - 1] == '\r')
      line.erase(line.size() - 1);
    if (line.empty() || line[0] == '#')
      continue;
    const size_t separator = line.find('=');
    if (separator == std::string::npos || separator == 0 ||
        separator + 1 >= line.size()) {
      if (error)
        *error = "invalid CHIP-8 config line " + std::to_string(line_number);
      return false;
    }
    const std::string key = line.substr(0, separator);
    const std::string value = line.substr(separator + 1);
    bool valid = true;
    if (key == "tickrate") {
      char *end = NULL;
      errno = 0;
      const long parsed = std::strtol(value.c_str(), &end, 10);
      valid = !errno && end && *end == '\0' && parsed >= 1 && parsed <= 50000;
      if (valid)
        configuration->core.tickrate = static_cast<int>(parsed);
    } else if (key == "shift_quirk") {
      valid = parse_boolean(value, &configuration->core.shift_quirk);
    } else if (key == "load_store_quirk") {
      valid = parse_boolean(value, &configuration->core.load_store_quirk);
    } else if (key == "jump_quirk") {
      valid = parse_boolean(value, &configuration->core.jump_quirk);
    } else if (key == "logic_quirk") {
      valid = parse_boolean(value, &configuration->core.logic_quirk);
    } else if (key == "clip_quirk") {
      valid = parse_boolean(value, &configuration->core.clip_quirk);
    } else if (key == "vblank_quirk") {
      valid = parse_boolean(value, &configuration->core.vblank_quirk);
    } else if (key.size() == 6 && key.compare(0, 5, "color") == 0 &&
               key[5] >= '0' && key[5] <= '3') {
      valid = parse_color(value,
                          &configuration->core.colors[key[5] - '0']);
    } else if (key == "input") {
      if (value == "octo")
        configuration->input = InputOcto;
      else if (value == "space-racer")
        configuration->input = InputSpaceRacer;
      else
        valid = false;
    } else {
      valid = false;
    }
    if (!valid) {
      if (error)
        *error = "invalid CHIP-8 config value on line " +
                 std::to_string(line_number);
      return false;
    }
  }
  if (input.bad()) {
    if (error)
      *error = "cannot read CHIP-8 config " + path;
    return false;
  }
  return true;
}

bool read_rom(const std::string &path, std::vector<uint8_t> *rom,
              std::string *error) {
  if (!rom)
    return false;
  struct stat info;
  if (stat(path.c_str(), &info) != 0) {
    if (error)
      *error = "cannot stat ROM " + path + ": " + std::strerror(errno);
    return false;
  }
  if (!S_ISREG(info.st_mode) || info.st_size <= 0 ||
      info.st_size > static_cast<off_t>(kMaximumRomBytes)) {
    if (error)
      *error = "CHIP-8 ROM must contain 1 through 65024 bytes";
    return false;
  }
  std::ifstream input(path.c_str(), std::ios::in | std::ios::binary);
  rom->resize(static_cast<size_t>(info.st_size));
  input.read(reinterpret_cast<char *>(rom->data()),
             static_cast<std::streamsize>(rom->size()));
  if (input.gcount() != static_cast<std::streamsize>(rom->size()) ||
      input.bad()) {
    rom->clear();
    if (error)
      *error = "cannot read complete CHIP-8 ROM " + path;
    return false;
  }
  return true;
}

void set_key(Chip8Core *core, unsigned int key, bool pressed) {
  Chip8CoreSetKey(core, key, pressed ? 1 : 0);
}

void update_input(Chip8Core *core, InputProfile profile) {
  const unsigned int first = GetJoypadInput(0);
  const unsigned int second = GetJoypadInput(1);
  bool keys[16] = {};

  if (profile == InputSpaceRacer) {
    keys[0x4] = (first & PAD_UP) != 0;
    keys[0x7] = (first & PAD_DOWN) != 0;
    keys[0xd] = (second & PAD_UP) != 0;
    keys[0xe] = (second & PAD_DOWN) != 0;
    keys[0xf] = (first & (PAD_A | PAD_START)) != 0;
  } else {
    keys[0x5] = (first & PAD_UP) != 0;    // W
    keys[0x8] = (first & PAD_DOWN) != 0;  // S
    keys[0x7] = (first & PAD_LEFT) != 0;  // A
    keys[0x9] = (first & PAD_RIGHT) != 0; // D
    keys[0x6] = (first & PAD_A) != 0;     // E
    keys[0x4] = (first & PAD_B) != 0;     // Q
    keys[0xa] = (first & PAD_SELECT) != 0;// Z
    keys[0xf] = (first & PAD_START) != 0; // V
  }
  for (unsigned int key = 0; key < 16; ++key)
    set_key(core, key, keys[key]);
}

void install_signal_handlers() {
  struct sigaction action;
  std::memset(&action, 0, sizeof(action));
  action.sa_handler = request_shutdown;
  sigemptyset(&action.sa_mask);
  sigaction(SIGINT, &action, NULL);
  sigaction(SIGTERM, &action, NULL);
}

} // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, NULL, _IOLBF, 0);
  if (argc != 2) {
    std::fprintf(stderr, "Usage: %s ROM.ch8\n", argv[0]);
    return 2;
  }
  install_signal_handlers();

  std::string error;
  std::vector<uint8_t> rom;
  if (!read_rom(argv[1], &rom, &error)) {
    std::fprintf(stderr, "chip8-deck: %s\n", error.c_str());
    return 1;
  }
  Configuration configuration;
  if (!parse_config(std::string(argv[1]) + ".cfg", &configuration, &error)) {
    std::fprintf(stderr, "chip8-deck: %s\n", error.c_str());
    return 1;
  }
  Chip8Core *core =
      Chip8CoreCreate(&rom[0], rom.size(), &configuration.core);
  if (!core) {
    std::fprintf(stderr, "chip8-deck: cannot initialize emulator core\n");
    return 1;
  }

  if (InitJoypadInput() < 0)
    std::fprintf(stderr, "chip8-deck: continuing without controller input\n");
  DeckFramebuffer framebuffer;
  if (!framebuffer.open_device(&error)) {
    std::fprintf(stderr, "chip8-deck: %s\n", error.c_str());
    Chip8CoreDestroy(core);
    return 1;
  }

  unsigned int volume = 42;
  if (!DeckReadVolumePercent(&volume, &error)) {
    std::fprintf(stderr, "chip8-deck: %s\n", error.c_str());
    Chip8CoreDestroy(core);
    return 1;
  }
  DeckAudio audio;
  if (!audio.open_device(44100, volume, &error))
    std::fprintf(stderr, "chip8-deck: sound disabled: %s\n", error.c_str());

  std::printf("chip8-deck: %zu-byte ROM, %d instructions/frame, volume %u%%\n",
              rom.size(), configuration.core.tickrate, volume);
  DeckFrameClock clock(60.0);
  const bool runtime_diagnostics =
      std::getenv("RETRO_DECK_RUNTIME_DIAGNOSTICS") != NULL;
  uint64_t frames = 0;
  struct timespec diagnostics_started;
  std::memset(&diagnostics_started, 0, sizeof(diagnostics_started));
  clock_gettime(CLOCK_MONOTONIC, &diagnostics_started);
  while (!shutdown_requested && !Chip8CoreHalted(core)) {
    update_input(core, configuration.input);
    const bool sound = Chip8CoreRunFrame(core) != 0;
    unsigned int width = 0;
    unsigned int height = 0;
    size_t pitch = 0;
    const uint8_t *pixels = Chip8CorePixels(core, &width, &height, &pitch);
    if (!framebuffer.present_indexed(pixels, width, height, pitch,
                                     Chip8CorePalette(core), 4, &error)) {
      std::fprintf(stderr, "chip8-deck: %s\n", error.c_str());
      Chip8CoreDestroy(core);
      return 1;
    }
    if (audio.available())
      audio.write_square_frame(sound);
    clock.wait_for_next_frame();
    ++frames;
    if (runtime_diagnostics && frames % 60 == 0) {
      struct timespec now;
      std::memset(&now, 0, sizeof(now));
      clock_gettime(CLOCK_MONOTONIC, &now);
      const double elapsed =
          static_cast<double>(now.tv_sec - diagnostics_started.tv_sec) +
          static_cast<double>(now.tv_nsec - diagnostics_started.tv_nsec) /
              1000000000.0;
      std::printf("chip8-deck: diagnostics video=60 wall=%.3f queued=%zu "
                  "dropped=%llu\n",
                  elapsed, audio.queued_frames(),
                  static_cast<unsigned long long>(audio.dropped_frames()));
      diagnostics_started = now;
    }
  }

  if (Chip8CoreHalted(core) && Chip8CoreHaltMessage(core)[0] != '\0')
    std::fprintf(stderr, "chip8-deck: halted: %s\n",
                 Chip8CoreHaltMessage(core));
  Chip8CoreDestroy(core);
  return 0;
}
