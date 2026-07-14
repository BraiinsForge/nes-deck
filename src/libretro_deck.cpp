#include "deck_runtime.h"
#if defined(RETRO_DECK_NES)
#include "nes_sram.h"
#endif

#include <libretro.h>

#ifndef RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2
#define RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 67
#endif
#ifndef RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL
#define RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL 68
#endif

#include <cerrno>
#include <cstdarg>
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

#if defined(RETRO_DECK_NES)
const char *const kFrontendName = "nes-deck";
const char *const kDefaultCoreName = "FCEUmm";
const char *const kRomDescription = "NES";
const char *const kRomUsage = "ROM.nes";
const char *const kSaveExtension = ".srm";
const size_t kMinimumRomBytes = 16;
const unsigned int kPlayerCount = 2;
const bool kHasRtc = false;
#elif defined(RETRO_DECK_GB)
const char *const kFrontendName = "gb-deck";
const char *const kDefaultCoreName = "Gambatte";
const char *const kRomDescription = "Game Boy";
const char *const kRomUsage = "ROM.gb|ROM.gbc";
const char *const kSaveExtension = ".sav";
const size_t kMinimumRomBytes = 0x150;
const unsigned int kPlayerCount = 1;
const bool kHasRtc = true;
#elif defined(RETRO_DECK_ZX)
const char *const kFrontendName = "zx-deck";
const char *const kDefaultCoreName = "Fuse";
const char *const kRomDescription = "ZX Spectrum";
const char *const kRomUsage = "ROM.tap";
const char *const kSaveExtension = ".sav";
const size_t kMinimumRomBytes = 4;
const unsigned int kPlayerCount = 2;
const bool kHasRtc = false;
#else
#error "Select exactly one libretro Deck frontend"
#endif

const unsigned int PAD_A = 1u << 0;
const unsigned int PAD_B = 1u << 1;
const unsigned int PAD_SELECT = 1u << 2;
const unsigned int PAD_START = 1u << 3;
const unsigned int PAD_UP = 1u << 4;
const unsigned int PAD_DOWN = 1u << 5;
const unsigned int PAD_LEFT = 1u << 6;
const unsigned int PAD_RIGHT = 1u << 7;
#if defined(RETRO_DECK_ZX)
const unsigned int PAD_L = 1u << 8;
const unsigned int PAD_R = 1u << 9;
#endif
const size_t kMaximumRomBytes = 8 * 1024 * 1024;

volatile sig_atomic_t shutdown_requested = 0;
DeckFramebuffer *framebuffer = NULL;
DeckAudio *audio = NULL;
std::string system_directory;
bool video_failed = false;
uint64_t audio_frames_received = 0;
uint64_t audio_callbacks_received = 0;
unsigned int video_divisor = 1;
uint64_t video_callbacks_received = 0;
enum retro_pixel_format video_pixel_format = RETRO_PIXEL_FORMAT_XRGB8888;

extern "C" int InitJoypadInput(void);
extern "C" unsigned int GetJoypadInput(unsigned int player);

void request_shutdown(int signal_number) {
  (void)signal_number;
  shutdown_requested = 1;
}

void core_log(enum retro_log_level level, const char *format, ...) {
  const char *prefix = level == RETRO_LOG_ERROR   ? "error"
                       : level == RETRO_LOG_WARN  ? "warning"
                       : level == RETRO_LOG_DEBUG ? "debug"
                                                  : "info";
  std::fprintf(stderr, "%s: core %s: ", kFrontendName, prefix);
  va_list arguments;
  va_start(arguments, format);
  std::vfprintf(stderr, format, arguments);
  va_end(arguments);
}

bool environment_callback(unsigned int command, void *data) {
  switch (command) {
  case RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION:
    if (data)
      *static_cast<unsigned int *>(data) = 2;
    return data != NULL;
  case RETRO_ENVIRONMENT_GET_LANGUAGE:
    if (data)
      *static_cast<unsigned int *>(data) = RETRO_LANGUAGE_ENGLISH;
    return data != NULL;
  case RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY:
  case RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY:
  case RETRO_ENVIRONMENT_GET_CONTENT_DIRECTORY:
    if (data)
      *static_cast<const char **>(data) = system_directory.c_str();
    return data != NULL;
  case RETRO_ENVIRONMENT_GET_LOG_INTERFACE:
    if (data)
      static_cast<struct retro_log_callback *>(data)->log = core_log;
    return data != NULL;
  case RETRO_ENVIRONMENT_GET_INPUT_BITMASKS:
    return true;
  case RETRO_ENVIRONMENT_GET_CAN_DUPE:
    if (data)
      *static_cast<bool *>(data) = true;
    return data != NULL;
  case RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE:
    if (data)
      *static_cast<bool *>(data) = false;
    return data != NULL;
  case RETRO_ENVIRONMENT_GET_VARIABLE:
#if defined(RETRO_DECK_NES)
    if (data) {
      struct retro_variable *variable = static_cast<struct retro_variable *>(data);
      if (!variable->key)
        return false;
      if (std::strcmp(variable->key, "fceumm_region") == 0)
        variable->value = "Auto";
      else if (std::strcmp(variable->key, "fceumm_overscan_h_left") == 0 ||
               std::strcmp(variable->key, "fceumm_overscan_h_right") == 0)
        variable->value = "0";
      else if (std::strcmp(variable->key, "fceumm_overscan_v_top") == 0 ||
               std::strcmp(variable->key, "fceumm_overscan_v_bottom") == 0)
        variable->value = "8";
      else
        variable->value = NULL;
      return variable->value != NULL;
    }
#elif defined(RETRO_DECK_ZX)
    if (data) {
      struct retro_variable *variable = static_cast<struct retro_variable *>(data);
      if (!variable->key)
        return false;
      if (std::strcmp(variable->key, "fuse_machine") == 0)
        variable->value = "Spectrum 48K";
      else if (std::strcmp(variable->key, "fuse_emulation_speed") == 0)
        variable->value = "100";
      else if (std::strcmp(variable->key, "fuse_size_border") == 0)
        variable->value = "full";
      else if (std::strcmp(variable->key, "fuse_palette") == 0)
        variable->value = "Fuse Standard";
      else if (std::strcmp(variable->key, "fuse_auto_load") == 0 ||
               std::strcmp(variable->key, "fuse_fast_load") == 0)
        variable->value = "enabled";
      else if (std::strcmp(variable->key, "fuse_load_sound") == 0 ||
               std::strcmp(variable->key, "fuse_display_joystick_type") == 0)
        variable->value = "disabled";
      else if (std::strcmp(variable->key, "fuse_speaker_type") == 0)
        variable->value = "tv speaker";
      else if (std::strcmp(variable->key, "fuse_ay_stereo_separation") == 0)
        variable->value = "none";
      else if (std::strcmp(variable->key, "fuse_key_ovrlay_transp") == 0)
        variable->value = "enabled";
      else if (std::strcmp(variable->key, "fuse_key_hold_time") == 0)
        variable->value = "500";
      else if (std::strcmp(variable->key, "fuse_joypad_start") == 0)
        variable->value = "Enter";
      else if (std::strncmp(variable->key, "fuse_joypad_", 12) == 0)
        variable->value = "<none>";
      else
        variable->value = NULL;
      return variable->value != NULL;
    }
#endif
    return false;
  case RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE:
    return false;
  case RETRO_ENVIRONMENT_SET_PIXEL_FORMAT:
    if (!data)
      return false;
    video_pixel_format = *static_cast<enum retro_pixel_format *>(data);
    return video_pixel_format == RETRO_PIXEL_FORMAT_XRGB8888 ||
           video_pixel_format == RETRO_PIXEL_FORMAT_RGB565;
  case RETRO_ENVIRONMENT_SET_CORE_OPTIONS:
  case RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL:
  case RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2:
  case RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL:
  case RETRO_ENVIRONMENT_SET_VARIABLES:
  case RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY:
  case RETRO_ENVIRONMENT_SET_CONTROLLER_INFO:
  case RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS:
  case RETRO_ENVIRONMENT_SET_MEMORY_MAPS:
  case RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO:
  case RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS:
  case RETRO_ENVIRONMENT_SET_GEOMETRY:
  case RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL:
    return true;
  default:
    return false;
  }
}

void video_callback(const void *data, unsigned int width, unsigned int height,
                    size_t pitch) {
  if (!data || !framebuffer || video_failed)
    return;
  ++video_callbacks_received;
  if (video_divisor > 1 && video_callbacks_received % video_divisor != 0)
    return;
  std::string error;
  const bool presented = video_pixel_format == RETRO_PIXEL_FORMAT_RGB565
                             ? framebuffer->present_rgb565(
                                   data, width, height, pitch, &error)
                             : framebuffer->present_xrgb8888(
                                   data, width, height, pitch, &error);
  if (!presented) {
    std::fprintf(stderr, "%s: video error: %s\n", kFrontendName,
                 error.c_str());
    video_failed = true;
  }
}

size_t audio_callback(const int16_t *data, size_t frames) {
  audio_frames_received += frames;
  ++audio_callbacks_received;
  if (audio && audio->available())
    audio->write_stereo(data, frames);
  return frames;
}

void input_poll_callback() {}

int16_t input_state_callback(unsigned int port, unsigned int device,
                             unsigned int index, unsigned int id) {
  if (port >= kPlayerCount || device != RETRO_DEVICE_JOYPAD || index != 0)
    return 0;
  const unsigned int state = GetJoypadInput(port);
  uint16_t result = 0;
  if (state & PAD_A)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_A;
  if (state & PAD_B)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_B;
  if (state & PAD_SELECT)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_SELECT;
  if (state & PAD_START)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_START;
  if (state & PAD_UP)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_UP;
  if (state & PAD_DOWN)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_DOWN;
  if (state & PAD_LEFT)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_LEFT;
  if (state & PAD_RIGHT)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_RIGHT;
#if defined(RETRO_DECK_ZX)
  if (state & PAD_L)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_L;
  if (state & PAD_R)
    result |= 1u << RETRO_DEVICE_ID_JOYPAD_R;
#endif
  if (id == RETRO_DEVICE_ID_JOYPAD_MASK)
    return static_cast<int16_t>(result);
  if (id > RETRO_DEVICE_ID_JOYPAD_R3)
    return 0;
  return (result & (1u << id)) != 0;
}

std::string parent_directory(const std::string &path) {
  const size_t separator = path.rfind('/');
  if (separator == std::string::npos)
    return ".";
  if (separator == 0)
    return "/";
  return path.substr(0, separator);
}

std::string save_base(const std::string &path) {
  const size_t separator = path.rfind('/');
  const size_t dot = path.rfind('.');
  if (dot == std::string::npos ||
      (separator != std::string::npos && dot < separator))
    return path;
  return path.substr(0, dot);
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
  if (!S_ISREG(info.st_mode) ||
      info.st_size < static_cast<off_t>(kMinimumRomBytes) ||
      info.st_size > static_cast<off_t>(kMaximumRomBytes)) {
    if (error)
      *error = std::string(kRomDescription) + " ROM has an invalid size";
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
      *error = "cannot read complete " + std::string(kRomDescription) +
               " ROM " + path;
    return false;
  }
#if defined(RETRO_DECK_NES)
  if (rom->size() < 4 || (*rom)[0] != 'N' || (*rom)[1] != 'E' ||
      (*rom)[2] != 'S' || (*rom)[3] != 0x1a) {
    rom->clear();
    if (error)
      *error = "NES ROM is missing its iNES header";
    return false;
  }
#endif
  return true;
}

bool write_all(int fd, const uint8_t *data, size_t size) {
  while (size > 0) {
    const ssize_t amount = write(fd, data, size);
    if (amount > 0) {
      data += amount;
      size -= static_cast<size_t>(amount);
    } else if (amount < 0 && errno == EINTR) {
      continue;
    } else {
      return false;
    }
  }
  return true;
}

bool save_memory_file(const std::string &path, const void *data, size_t size) {
  if (!data || size == 0)
    return true;
  const std::string temporary = path + ".tmp." + std::to_string(getpid());
  const int fd = open(temporary.c_str(),
                      O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
  if (fd < 0)
    return false;
  bool ok = write_all(fd, static_cast<const uint8_t *>(data), size);
  if (ok)
    ok = fsync(fd) == 0;
  if (close(fd) != 0)
    ok = false;
  if (ok)
    ok = rename(temporary.c_str(), path.c_str()) == 0;
  if (!ok)
    unlink(temporary.c_str());
  return ok;
}

void load_memory_file(const std::string &path, void *data, size_t size) {
  if (!data || size == 0)
    return;
  struct stat info;
  if (stat(path.c_str(), &info) != 0) {
    if (errno != ENOENT)
      std::fprintf(stderr, "%s: cannot stat save %s: %s\n", kFrontendName,
                   path.c_str(), std::strerror(errno));
    return;
  }
  if (!S_ISREG(info.st_mode)) {
    std::fprintf(stderr, "%s: save is not a regular file: %s\n",
                 kFrontendName, path.c_str());
    return;
  }
  if (info.st_size != static_cast<off_t>(size)) {
#if defined(RETRO_DECK_NES)
    if (info.st_size > 0 &&
        static_cast<size_t>(info.st_size) <= NesSramMaximumEncodedSize(size)) {
      std::ifstream input(path.c_str(), std::ios::in | std::ios::binary);
      std::vector<uint8_t> encoded(static_cast<size_t>(info.st_size));
      input.read(reinterpret_cast<char *>(encoded.data()),
                 static_cast<std::streamsize>(encoded.size()));
      if (input.gcount() == static_cast<std::streamsize>(encoded.size()) &&
          !input.bad() &&
          NesSramDecode(encoded.data(), encoded.size(),
                        static_cast<uint8_t *>(data), size)) {
        std::fprintf(stderr, "%s: migrated encoded InfoNES save: %s\n",
                     kFrontendName, path.c_str());
        return;
      }
    }
#endif
    std::fprintf(stderr,
                 "%s: ignoring save with unexpected size: %s\n",
                 kFrontendName, path.c_str());
    return;
  }
  const int fd = open(path.c_str(), O_RDONLY | O_CLOEXEC);
  if (fd < 0)
    return;
  uint8_t *destination = static_cast<uint8_t *>(data);
  size_t remaining = size;
  while (remaining > 0) {
    const ssize_t amount = read(fd, destination, remaining);
    if (amount > 0) {
      destination += amount;
      remaining -= static_cast<size_t>(amount);
    } else if (amount < 0 && errno == EINTR) {
      continue;
    } else {
      break;
    }
  }
  close(fd);
  if (remaining != 0)
    std::fprintf(stderr, "%s: incomplete save read: %s\n", kFrontendName,
                 path.c_str());
}

void load_persistent_memory(const std::string &base) {
  load_memory_file(base + kSaveExtension,
                   retro_get_memory_data(RETRO_MEMORY_SAVE_RAM),
                   retro_get_memory_size(RETRO_MEMORY_SAVE_RAM));
  if (kHasRtc)
    load_memory_file(base + ".rtc", retro_get_memory_data(RETRO_MEMORY_RTC),
                     retro_get_memory_size(RETRO_MEMORY_RTC));
}

bool save_persistent_memory(const std::string &base) {
  const bool ram =
      save_memory_file(base + kSaveExtension,
                       retro_get_memory_data(RETRO_MEMORY_SAVE_RAM),
                       retro_get_memory_size(RETRO_MEMORY_SAVE_RAM));
  const bool rtc =
      !kHasRtc || save_memory_file(base + ".rtc",
                                  retro_get_memory_data(RETRO_MEMORY_RTC),
                                  retro_get_memory_size(RETRO_MEMORY_RTC));
  return ram && rtc;
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
    std::fprintf(stderr, "Usage: %s %s\n", argv[0], kRomUsage);
    return 2;
  }
  install_signal_handlers();

  std::string error;
  std::vector<uint8_t> rom;
  if (!read_rom(argv[1], &rom, &error)) {
    std::fprintf(stderr, "%s: %s\n", kFrontendName, error.c_str());
    return 1;
  }

  system_directory = parent_directory(argv[1]);
  retro_set_environment(environment_callback);
  retro_set_video_refresh(video_callback);
  retro_set_audio_sample(NULL);
  retro_set_audio_sample_batch(audio_callback);
  retro_set_input_poll(input_poll_callback);
  retro_set_input_state(input_state_callback);
  retro_init();
#if defined(RETRO_DECK_ZX)
  retro_set_controller_port_device(
      0, RETRO_DEVICE_SUBCLASS(RETRO_DEVICE_JOYPAD, 1));
  retro_set_controller_port_device(
      1, RETRO_DEVICE_SUBCLASS(RETRO_DEVICE_JOYPAD, 3));
#endif

  struct retro_system_info system_info;
  std::memset(&system_info, 0, sizeof(system_info));
  retro_get_system_info(&system_info);
  if (retro_api_version() != RETRO_API_VERSION) {
    std::fprintf(stderr, "%s: incompatible libretro API\n", kFrontendName);
    retro_deinit();
    return 1;
  }

  struct retro_game_info game;
  std::memset(&game, 0, sizeof(game));
  game.path = argv[1];
  game.data = &rom[0];
  game.size = rom.size();
  if (!retro_load_game(&game)) {
    std::fprintf(stderr, "%s: %s core rejected the ROM\n", kFrontendName,
                 kRomDescription);
    retro_deinit();
    return 1;
  }

  if (InitJoypadInput() < 0)
    std::fprintf(stderr, "%s: continuing without controller input\n",
                 kFrontendName);
  DeckFramebuffer deck_framebuffer;
  if (!deck_framebuffer.open_device(&error)) {
    std::fprintf(stderr, "%s: %s\n", kFrontendName, error.c_str());
    retro_unload_game();
    retro_deinit();
    return 1;
  }
  framebuffer = &deck_framebuffer;

  struct retro_system_av_info av_info;
  std::memset(&av_info, 0, sizeof(av_info));
  retro_get_system_av_info(&av_info);
  const unsigned int sample_rate =
      static_cast<unsigned int>(av_info.timing.sample_rate + 0.5);
  unsigned int volume = 42;
  if (!DeckReadVolumePercent(&volume, &error)) {
    std::fprintf(stderr, "%s: %s\n", kFrontendName, error.c_str());
    retro_unload_game();
    retro_deinit();
    return 1;
  }
  DeckAudio deck_audio;
  if (!deck_audio.open_device(sample_rate, volume, &error))
    std::fprintf(stderr, "%s: sound disabled: %s\n", kFrontendName,
                 error.c_str());
  audio = &deck_audio;

  const std::string persistent_base = save_base(argv[1]);
  load_persistent_memory(persistent_base);
  std::printf("%s: %s %s, %.3f fps, %u Hz, volume %u%%\n", kFrontendName,
              system_info.library_name ? system_info.library_name
                                       : kDefaultCoreName,
              system_info.library_version ? system_info.library_version : "",
              av_info.timing.fps, sample_rate, volume);

  DeckFrameClock clock(av_info.timing.fps);
  uint64_t frames = 0;
  const bool runtime_diagnostics =
      std::getenv("RETRO_DECK_RUNTIME_DIAGNOSTICS") != NULL;
  if (runtime_diagnostics) {
    const char *divisor_text = std::getenv("RETRO_DECK_VIDEO_DIVISOR");
    if (divisor_text && *divisor_text) {
      char *end = NULL;
      errno = 0;
      const unsigned long parsed = std::strtoul(divisor_text, &end, 10);
      if (!errno && end && *end == '\0' && parsed >= 1 && parsed <= 60)
        video_divisor = static_cast<unsigned int>(parsed);
    }
  }
  struct timespec diagnostics_started;
  std::memset(&diagnostics_started, 0, sizeof(diagnostics_started));
  clock_gettime(CLOCK_MONOTONIC, &diagnostics_started);
  uint64_t previous_audio_frames = 0;
  uint64_t previous_audio_callbacks = 0;
  while (!shutdown_requested && !video_failed) {
    retro_run();
    ++frames;
    if (runtime_diagnostics && frames % 60 == 0) {
      struct timespec now;
      std::memset(&now, 0, sizeof(now));
      clock_gettime(CLOCK_MONOTONIC, &now);
      const double elapsed =
          static_cast<double>(now.tv_sec - diagnostics_started.tv_sec) +
          static_cast<double>(now.tv_nsec - diagnostics_started.tv_nsec) /
              1000000000.0;
      std::printf("%s: diagnostics video=60 wall=%.3f audio=%llu "
                  "callbacks=%llu queued=%zu dropped=%llu\n",
                  kFrontendName, elapsed,
                  static_cast<unsigned long long>(audio_frames_received -
                                                  previous_audio_frames),
                  static_cast<unsigned long long>(audio_callbacks_received -
                                                  previous_audio_callbacks),
                  audio ? audio->queued_frames() : 0,
                  static_cast<unsigned long long>(
                      audio ? audio->dropped_frames() : 0));
      diagnostics_started = now;
      previous_audio_frames = audio_frames_received;
      previous_audio_callbacks = audio_callbacks_received;
    }
    if (frames % 600 == 0 && !save_persistent_memory(persistent_base))
      std::fprintf(stderr, "%s: periodic save failed: %s\n", kFrontendName,
                   std::strerror(errno));
    clock.wait_for_next_frame();
  }

  if (!save_persistent_memory(persistent_base))
    std::fprintf(stderr, "%s: final save failed: %s\n", kFrontendName,
                 std::strerror(errno));
  audio = NULL;
  framebuffer = NULL;
  retro_unload_game();
  retro_deinit();
  return video_failed ? 1 : 0;
}
