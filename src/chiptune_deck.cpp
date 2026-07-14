#include "deck_runtime.h"

#include <gme/gme.h>
#include <vorbis/vorbisfile.h>

#include <algorithm>
#include <cerrno>
#include <climits>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dirent.h>
#include <fcntl.h>
#include <linux/input.h>
#include <string>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <unistd.h>
#include <vector>

namespace {

const int kLogicalWidth = 1280;
const int kLogicalHeight = 480;
const int kCanvasWidth = 624;
const int kCanvasHeight = 224;
const int kCanvasOffset = 16;
const int kCanvasScale = 2;
const unsigned int kSampleRate = 44100;
const size_t kFramesPerTick = 735;
const size_t kMaximumFiles = 1024;
const off_t kMaximumFileSize = 16 * 1024 * 1024;
const unsigned short kTheGamepadVendor = 0x1c59;
const unsigned short kTheGamepadProduct = 0x0026;

volatile sig_atomic_t shutdown_requested = 0;

struct Rect {
  int x;
  int y;
  int width;
  int height;
};

const Rect kCloseButton = {554, 3, 62, 34};
const Rect kPreviousFileButton = {8, 66, 62, 82};
const Rect kNextFileButton = {554, 66, 62, 82};
const Rect kPlaybackModeButton = {113, 177, 92, 34};
const Rect kPreviousTrackButton = {215, 177, 92, 34};
const Rect kPauseButton = {317, 177, 92, 34};
const Rect kNextTrackButton = {419, 177, 92, 34};

typedef std::vector<uint16_t> Canvas;

enum ControlCommand {
  ControlNone = 0,
  ControlBack = 1 << 0,
  ControlPreviousFile = 1 << 1,
  ControlNextFile = 1 << 2,
  ControlTogglePause = 1 << 3,
  ControlPreviousTrack = 1 << 4,
  ControlNextTrack = 1 << 5,
  ControlVolumeDown = 1 << 6,
  ControlVolumeUp = 1 << 7,
  ControlCyclePlaybackMode = 1 << 8
};

void request_shutdown(int signal_number) {
  (void)signal_number;
  shutdown_requested = 1;
}

void install_signal_handlers() {
  struct sigaction action;
  std::memset(&action, 0, sizeof(action));
  action.sa_handler = request_shutdown;
  sigemptyset(&action.sa_mask);
  sigaction(SIGINT, &action, NULL);
  sigaction(SIGTERM, &action, NULL);
}

bool bit_is_set(const unsigned long *bits, unsigned int bit) {
  const unsigned int bits_per_word = sizeof(unsigned long) * CHAR_BIT;
  return (bits[bit / bits_per_word] & (1UL << (bit % bits_per_word))) != 0;
}

bool contains(const Rect &rect, int x, int y) {
  return x >= rect.x && x < rect.x + rect.width && y >= rect.y &&
         y < rect.y + rect.height;
}

void fill_rect(Canvas *canvas, const Rect &rect, uint16_t color) {
  if (!canvas || canvas->size() !=
                     static_cast<size_t>(kCanvasWidth * kCanvasHeight))
    return;
  const int left = std::max(0, rect.x);
  const int top = std::max(0, rect.y);
  const int right = std::min(kCanvasWidth, rect.x + rect.width);
  const int bottom = std::min(kCanvasHeight, rect.y + rect.height);
  for (int y = top; y < bottom; ++y) {
    std::fill(canvas->begin() + y * kCanvasWidth + left,
              canvas->begin() + y * kCanvasWidth + right, color);
  }
}

const uint8_t *glyph_rows(char input) {
  static const uint8_t space[7] = {0, 0, 0, 0, 0, 0, 0};
  static const uint8_t digits[10][7] = {
      {14, 17, 19, 21, 25, 17, 14}, {4, 12, 4, 4, 4, 4, 14},
      {14, 17, 1, 2, 4, 8, 31},     {30, 1, 1, 14, 1, 1, 30},
      {2, 6, 10, 18, 31, 2, 2},     {31, 16, 16, 30, 1, 1, 30},
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
  static const uint8_t colon[7] = {0, 6, 6, 0, 6, 6, 0};
  static const uint8_t hyphen[7] = {0, 0, 0, 31, 0, 0, 0};
  static const uint8_t plus[7] = {0, 4, 4, 31, 4, 4, 0};
  static const uint8_t slash[7] = {1, 2, 2, 4, 8, 8, 16};
  static const uint8_t unknown[7] = {14, 17, 1, 2, 4, 0, 4};
  unsigned char character = static_cast<unsigned char>(input);
  if (character >= 'a' && character <= 'z')
    character = static_cast<unsigned char>(character - 'a' + 'A');
  if (character >= 'A' && character <= 'Z')
    return letters[character - 'A'];
  if (character >= '0' && character <= '9')
    return digits[character - '0'];
  if (character == ' ')
    return space;
  if (character == '.')
    return period;
  if (character == ':')
    return colon;
  if (character == '-')
    return hyphen;
  if (character == '+')
    return plus;
  if (character == '/')
    return slash;
  return unknown;
}

void draw_text(Canvas *canvas, int x, int y, const std::string &text,
               int scale, uint16_t color) {
  for (size_t index = 0; index < text.size(); ++index) {
    const uint8_t *rows = glyph_rows(text[index]);
    for (int row = 0; row < 7; ++row) {
      for (int column = 0; column < 5; ++column) {
        if (rows[row] & (1u << (4 - column))) {
          fill_rect(canvas,
                    Rect{x + static_cast<int>(index) * 6 * scale +
                             column * scale,
                         y + row * scale, scale, scale},
                    color);
        }
      }
    }
  }
}

void draw_centered_text(Canvas *canvas, int y, const std::string &text,
                        int scale, uint16_t color) {
  const int width = text.empty()
                        ? 0
                        : static_cast<int>(text.size() * 6 - 1) * scale;
  draw_text(canvas, std::max(0, (kCanvasWidth - width) / 2), y, text, scale,
            color);
}

std::string uppercase_ascii(const std::string &input) {
  std::string result;
  for (size_t index = 0; index < input.size(); ++index) {
    unsigned char character = static_cast<unsigned char>(input[index]);
    if (character >= 'a' && character <= 'z')
      character = static_cast<unsigned char>(character - 'a' + 'A');
    if ((character >= 'A' && character <= 'Z') ||
        (character >= '0' && character <= '9') || character == ' ' ||
        character == '.' || character == ':' || character == '-' ||
        character == '+' || character == '/') {
      result.push_back(static_cast<char>(character));
    } else if (character == '_' || character == '\t') {
      result.push_back(' ');
    }
  }
  return result;
}

std::string clipped(const std::string &input, size_t maximum) {
  const std::string clean = uppercase_ascii(input);
  if (clean.size() <= maximum)
    return clean;
  if (maximum <= 3)
    return clean.substr(0, maximum);
  return clean.substr(0, maximum - 3) + "...";
}

std::string base_name(const std::string &path) {
  const size_t slash = path.find_last_of('/');
  std::string name = slash == std::string::npos ? path : path.substr(slash + 1);
  const size_t dot = name.find_last_of('.');
  if (dot != std::string::npos)
    name.erase(dot);
  std::replace(name.begin(), name.end(), '-', ' ');
  std::replace(name.begin(), name.end(), '_', ' ');
  return name;
}

std::string lower_extension(const std::string &path) {
  const size_t dot = path.find_last_of('.');
  if (dot == std::string::npos)
    return std::string();
  std::string extension = path.substr(dot);
  for (size_t index = 0; index < extension.size(); ++index) {
    if (extension[index] >= 'A' && extension[index] <= 'Z')
      extension[index] = static_cast<char>(extension[index] - 'A' + 'a');
  }
  return extension;
}

bool supported_extension(const std::string &path) {
  static const char *extensions[] = {".ay",  ".gbs", ".gym", ".hes",
                                     ".kss", ".nsf", ".nsfe", ".sap",
                                     ".spc", ".vgm", ".vgz",  ".ogg"};
  const std::string extension = lower_extension(path);
  for (size_t index = 0; index < sizeof(extensions) / sizeof(extensions[0]);
       ++index) {
    if (extension == extensions[index])
      return true;
  }
  return false;
}

void scan_chiptunes(const std::string &directory, unsigned int depth,
                    std::vector<std::string> *files) {
  if (!files || depth > 4 || files->size() >= kMaximumFiles)
    return;
  DIR *handle = opendir(directory.c_str());
  if (!handle)
    return;
  std::vector<std::string> names;
  for (struct dirent *entry = readdir(handle); entry; entry = readdir(handle)) {
    const std::string name(entry->d_name);
    if (name.empty() || name[0] == '.')
      continue;
    names.push_back(name);
  }
  closedir(handle);
  std::sort(names.begin(), names.end());
  for (size_t index = 0;
       index < names.size() && files->size() < kMaximumFiles; ++index) {
    const std::string path = directory + "/" + names[index];
    struct stat info;
    if (lstat(path.c_str(), &info) != 0)
      continue;
    if (S_ISDIR(info.st_mode)) {
      scan_chiptunes(path, depth + 1, files);
    } else if (S_ISREG(info.st_mode) && info.st_size > 0 &&
               info.st_size <= kMaximumFileSize && supported_extension(path)) {
      files->push_back(path);
    }
  }
}

bool read_chiptune(const std::string &path, std::vector<unsigned char> *bytes,
                   std::string *error) {
  if (!bytes)
    return false;
  bytes->clear();
  const int fd = open(path.c_str(), O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
  if (fd < 0) {
    if (error)
      *error = std::string("cannot open file: ") + std::strerror(errno);
    return false;
  }
  struct stat info;
  if (fstat(fd, &info) != 0 || !S_ISREG(info.st_mode) || info.st_size <= 0 ||
      info.st_size > kMaximumFileSize) {
    if (error)
      *error = "file is empty, oversized, or not regular";
    close(fd);
    return false;
  }
  bytes->resize(static_cast<size_t>(info.st_size));
  size_t offset = 0;
  while (offset < bytes->size()) {
    const ssize_t amount =
        read(fd, &(*bytes)[offset], bytes->size() - offset);
    if (amount > 0) {
      offset += static_cast<size_t>(amount);
    } else if (amount < 0 && errno == EINTR) {
      continue;
    } else {
      if (error)
        *error = amount == 0 ? "file ended during read"
                             : std::string("cannot read file: ") +
                                   std::strerror(errno);
      close(fd);
      bytes->clear();
      return false;
    }
  }
  if (close(fd) != 0) {
    if (error)
      *error = std::string("cannot close file: ") + std::strerror(errno);
    bytes->clear();
    return false;
  }
  return true;
}

bool save_player_volume(const std::string &path, unsigned int volume,
                        std::string *error) {
  if (path.empty())
    return true;
  if (path[0] != '/' || path.size() > static_cast<size_t>(PATH_MAX) ||
      volume > 100) {
    if (error)
      *error = "volume state path or value is invalid";
    return false;
  }
  const size_t slash = path.find_last_of('/');
  if (slash == std::string::npos || slash + 1 >= path.size()) {
    if (error)
      *error = "volume state path has no filename";
    return false;
  }
  const std::string directory = slash == 0 ? "/" : path.substr(0, slash);
  const std::string filename = path.substr(slash + 1);
  const int directory_fd =
      open(directory.c_str(), O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
  if (directory_fd < 0) {
    if (error)
      *error = std::string("cannot open volume state directory: ") +
               std::strerror(errno);
    return false;
  }

  int state_fd = -1;
  std::string temporary;
  for (unsigned int attempt = 0; attempt < 8 && state_fd < 0; ++attempt) {
    char suffix[64];
    std::snprintf(suffix, sizeof(suffix), ".chiptune.%ld.%u",
                  static_cast<long>(getpid()), attempt);
    temporary = filename + suffix;
    if (temporary.size() > static_cast<size_t>(NAME_MAX))
      break;
    state_fd = openat(directory_fd, temporary.c_str(),
                      O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW,
                      0600);
    if (state_fd < 0 && errno != EEXIST)
      break;
  }
  if (state_fd < 0) {
    if (error)
      *error = std::string("cannot create volume state: ") +
               std::strerror(errno);
    close(directory_fd);
    return false;
  }

  const std::string value = std::to_string(volume) + "\n";
  size_t written = 0;
  while (written < value.size()) {
    const ssize_t amount =
        write(state_fd, value.data() + written, value.size() - written);
    if (amount > 0) {
      written += static_cast<size_t>(amount);
    } else if (amount < 0 && errno == EINTR) {
      continue;
    } else {
      break;
    }
  }
  bool valid = written == value.size() && fsync(state_fd) == 0;
  if (close(state_fd) != 0)
    valid = false;
  if (valid &&
      renameat(directory_fd, temporary.c_str(), directory_fd,
               filename.c_str()) == 0) {
    valid = fsync(directory_fd) == 0;
  } else {
    valid = false;
  }
  if (!valid) {
    const int saved_errno = errno;
    unlinkat(directory_fd, temporary.c_str(), 0);
    if (error)
      *error = std::string("cannot save volume state: ") +
               std::strerror(saved_errno);
  }
  close(directory_fd);
  return valid;
}

unsigned int axis_buttons(int value, const struct input_absinfo &info,
                          unsigned int negative, unsigned int positive) {
  if (info.maximum <= info.minimum)
    return 0;
  const int64_t span = static_cast<int64_t>(info.maximum) - info.minimum;
  if (value <= info.minimum + span / 3)
    return negative;
  if (value >= info.maximum - span / 3)
    return positive;
  return 0;
}

unsigned int gamepad_key(unsigned short code) {
  switch (code) {
  case BTN_THUMB2:
  case BTN_TOP:
    return ControlTogglePause;
  case BTN_THUMB:
  case BTN_TRIGGER:
    return ControlBack;
  case BTN_TOP2:
    return ControlPreviousTrack;
  case BTN_PINKIE:
    return ControlNextTrack;
  case BTN_BASE2:
    return ControlCyclePlaybackMode;
  default:
    return 0;
  }
}

struct GamepadDevice {
  int fd;
  struct input_absinfo x_info;
  struct input_absinfo y_info;
  int x_value;
  int y_value;
  uint32_t raw_buttons;
  unsigned int state;
  bool dropping;

  GamepadDevice()
      : fd(-1), x_value(0), y_value(0), raw_buttons(0), state(0),
        dropping(false) {
    std::memset(&x_info, 0, sizeof(x_info));
    std::memset(&y_info, 0, sizeof(y_info));
  }
};

unsigned int gamepad_state(const GamepadDevice &device) {
  unsigned int state = 0;
  for (unsigned int index = 0; index < 8; ++index) {
    if (device.raw_buttons & (1u << index))
      state |= gamepad_key(static_cast<unsigned short>(BTN_TRIGGER + index));
  }
  state |= axis_buttons(device.x_value, device.x_info, ControlPreviousFile,
                        ControlNextFile);
  state |= axis_buttons(device.y_value, device.y_info, ControlVolumeUp,
                        ControlVolumeDown);
  return state;
}

class PlayerInput {
public:
  PlayerInput()
      : touch_fd_(-1), touch_x_(0), touch_y_(0), touch_down_(false),
        touch_reported_(false), touch_dropping_(false), touch_grabbed_(false) {}
  ~PlayerInput() { close_devices(); }

  void discover() {
    close_devices();
    DIR *directory = opendir("/dev/input");
    if (!directory)
      return;
    std::vector<std::string> paths;
    for (struct dirent *entry = readdir(directory); entry;
         entry = readdir(directory)) {
      const std::string name(entry->d_name);
      if (name.size() > 5 && name.compare(0, 5, "event") == 0)
        paths.push_back("/dev/input/" + name);
    }
    closedir(directory);
    std::sort(paths.begin(), paths.end());
    for (size_t index = 0; index < paths.size(); ++index) {
      const int fd = open(paths[index].c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
      if (fd < 0)
        continue;
      char name[256] = {};
      struct input_id identity;
      std::memset(&identity, 0, sizeof(identity));
      ioctl(fd, EVIOCGNAME(sizeof(name)), name);
      ioctl(fd, EVIOCGID, &identity);
      if (touch_fd_ < 0 &&
          std::string(name).find("Goodix Capacitive TouchScreen") !=
              std::string::npos) {
        struct input_absinfo x_info;
        struct input_absinfo y_info;
        if (ioctl(fd, EVIOCGABS(ABS_X), &x_info) == 0 &&
            ioctl(fd, EVIOCGABS(ABS_Y), &y_info) == 0) {
          touch_fd_ = fd;
          touch_x_ = x_info.value;
          touch_y_ = y_info.value;
          touch_grabbed_ = ioctl(fd, EVIOCGRAB, 1) == 0;
          continue;
        }
      }
      if (identity.vendor == kTheGamepadVendor &&
          identity.product == kTheGamepadProduct && gamepads_.size() < 2) {
        GamepadDevice device;
        device.fd = fd;
        if (ioctl(fd, EVIOCGABS(ABS_X), &device.x_info) == 0 &&
            ioctl(fd, EVIOCGABS(ABS_Y), &device.y_info) == 0 &&
            resynchronize(&device)) {
          gamepads_.push_back(device);
          continue;
        }
      }
      close(fd);
    }
  }

  unsigned int read_commands() {
    unsigned int commands = 0;
    if (touch_fd_ >= 0)
      drain_touch(&commands);
    for (size_t index = 0; index < gamepads_.size(); ++index)
      drain_gamepad(&gamepads_[index], &commands);
    return commands;
  }

private:
  static bool resynchronize(GamepadDevice *device) {
    if (!device || device->fd < 0)
      return false;
    const size_t words = (KEY_MAX + sizeof(unsigned long) * CHAR_BIT) /
                         (sizeof(unsigned long) * CHAR_BIT);
    std::vector<unsigned long> keys(words, 0);
    if (ioctl(device->fd, EVIOCGABS(ABS_X), &device->x_info) != 0 ||
        ioctl(device->fd, EVIOCGABS(ABS_Y), &device->y_info) != 0 ||
        ioctl(device->fd,
              EVIOCGKEY(keys.size() * sizeof(unsigned long)), &keys[0]) < 0)
      return false;
    device->x_value = device->x_info.value;
    device->y_value = device->y_info.value;
    device->raw_buttons = 0;
    for (unsigned int index = 0; index < 8; ++index) {
      if (bit_is_set(&keys[0], BTN_TRIGGER + index))
        device->raw_buttons |= 1u << index;
    }
    device->state = gamepad_state(*device);
    device->dropping = false;
    return true;
  }

  static void drain_gamepad(GamepadDevice *device, unsigned int *commands) {
    if (!device || device->fd < 0 || !commands)
      return;
    while (true) {
      struct input_event events[32];
      const ssize_t amount = read(device->fd, events, sizeof(events));
      if (amount < 0) {
        if (errno == EINTR)
          continue;
        return;
      }
      if (amount == 0 ||
          amount % static_cast<ssize_t>(sizeof(struct input_event)) != 0)
        return;
      const size_t count = static_cast<size_t>(amount) / sizeof(events[0]);
      for (size_t index = 0; index < count; ++index) {
        const struct input_event &event = events[index];
        if (device->dropping) {
          if (event.type == EV_SYN && event.code == SYN_REPORT)
            resynchronize(device);
          continue;
        }
        if (event.type == EV_SYN && event.code == SYN_DROPPED) {
          device->dropping = true;
        } else if (event.type == EV_KEY && event.code >= BTN_TRIGGER &&
                   event.code <= BTN_BASE2) {
          const uint32_t bit = 1u << (event.code - BTN_TRIGGER);
          if (event.value)
            device->raw_buttons |= bit;
          else
            device->raw_buttons &= ~bit;
        } else if (event.type == EV_ABS && event.code == ABS_X) {
          device->x_value = event.value;
        } else if (event.type == EV_ABS && event.code == ABS_Y) {
          device->y_value = event.value;
        } else if (event.type == EV_SYN && event.code == SYN_REPORT) {
          const unsigned int state = gamepad_state(*device);
          *commands |= state & ~device->state;
          device->state = state;
        }
      }
    }
  }

  static unsigned int touch_command(int logical_x, int logical_y) {
    const int x = (logical_x - kCanvasOffset) / kCanvasScale;
    const int y = (logical_y - kCanvasOffset) / kCanvasScale;
    if (contains(kCloseButton, x, y))
      return ControlBack;
    if (contains(kPreviousFileButton, x, y))
      return ControlPreviousFile;
    if (contains(kPauseButton, x, y))
      return ControlTogglePause;
    if (contains(kNextFileButton, x, y))
      return ControlNextFile;
    if (contains(kPlaybackModeButton, x, y))
      return ControlCyclePlaybackMode;
    if (contains(kPreviousTrackButton, x, y))
      return ControlPreviousTrack;
    if (contains(kNextTrackButton, x, y))
      return ControlNextTrack;
    return ControlNone;
  }

  void drain_touch(unsigned int *commands) {
    while (true) {
      struct input_event events[32];
      const ssize_t amount = read(touch_fd_, events, sizeof(events));
      if (amount < 0) {
        if (errno == EINTR)
          continue;
        return;
      }
      if (amount == 0 ||
          amount % static_cast<ssize_t>(sizeof(struct input_event)) != 0)
        return;
      const size_t count = static_cast<size_t>(amount) / sizeof(events[0]);
      for (size_t index = 0; index < count; ++index) {
        const struct input_event &event = events[index];
        if (event.type == EV_SYN && event.code == SYN_DROPPED) {
          touch_dropping_ = true;
        } else if (touch_dropping_) {
          if (event.type == EV_SYN && event.code == SYN_REPORT) {
            touch_dropping_ = false;
            touch_reported_ = touch_down_;
          }
        } else if (event.type == EV_ABS && event.code == ABS_X) {
          touch_x_ = std::max(0, std::min(kLogicalWidth - 1, event.value));
        } else if (event.type == EV_ABS && event.code == ABS_Y) {
          touch_y_ = std::max(0, std::min(kLogicalHeight - 1, event.value));
        } else if (event.type == EV_KEY && event.code == BTN_TOUCH) {
          touch_down_ = event.value != 0;
        } else if (event.type == EV_SYN && event.code == SYN_REPORT) {
          if (touch_down_ && !touch_reported_)
            *commands |= touch_command(touch_x_, touch_y_);
          touch_reported_ = touch_down_;
        }
      }
    }
  }

  void close_devices() {
    if (touch_fd_ >= 0) {
      if (touch_grabbed_)
        ioctl(touch_fd_, EVIOCGRAB, 0);
      close(touch_fd_);
    }
    touch_fd_ = -1;
    touch_grabbed_ = false;
    for (size_t index = 0; index < gamepads_.size(); ++index) {
      if (gamepads_[index].fd >= 0)
        close(gamepads_[index].fd);
    }
    gamepads_.clear();
  }

  int touch_fd_;
  int touch_x_;
  int touch_y_;
  bool touch_down_;
  bool touch_reported_;
  bool touch_dropping_;
  bool touch_grabbed_;
  std::vector<GamepadDevice> gamepads_;
};

struct VorbisMemoryStream {
  const std::vector<unsigned char> *bytes;
  size_t offset;

  VorbisMemoryStream() : bytes(NULL), offset(0) {}
};

size_t vorbis_memory_read(void *destination, size_t size, size_t count,
                          void *datasource) {
  VorbisMemoryStream *stream = static_cast<VorbisMemoryStream *>(datasource);
  if (!stream || !stream->bytes || !destination || size == 0 || count == 0 ||
      count > SIZE_MAX / size)
    return 0;
  const size_t requested = size * count;
  const size_t available = stream->offset < stream->bytes->size()
                               ? stream->bytes->size() - stream->offset
                               : 0;
  const size_t complete_items = std::min(requested, available) / size;
  const size_t amount = complete_items * size;
  if (amount > 0) {
    std::memcpy(destination, &(*stream->bytes)[stream->offset], amount);
    stream->offset += amount;
  }
  return complete_items;
}

int vorbis_memory_seek(void *datasource, ogg_int64_t offset, int whence) {
  VorbisMemoryStream *stream = static_cast<VorbisMemoryStream *>(datasource);
  if (!stream || !stream->bytes)
    return -1;
  ogg_int64_t base = 0;
  if (whence == SEEK_CUR)
    base = static_cast<ogg_int64_t>(stream->offset);
  else if (whence == SEEK_END)
    base = static_cast<ogg_int64_t>(stream->bytes->size());
  else if (whence != SEEK_SET)
    return -1;
  if ((offset < 0 && base < -offset) ||
      (offset > 0 && base > static_cast<ogg_int64_t>(stream->bytes->size()) -
                                 offset))
    return -1;
  const ogg_int64_t target = base + offset;
  if (target < 0 ||
      target > static_cast<ogg_int64_t>(stream->bytes->size()))
    return -1;
  stream->offset = static_cast<size_t>(target);
  return 0;
}

int vorbis_memory_close(void *datasource) {
  (void)datasource;
  return 0;
}

long vorbis_memory_tell(void *datasource) {
  VorbisMemoryStream *stream = static_cast<VorbisMemoryStream *>(datasource);
  if (!stream || stream->offset > static_cast<size_t>(LONG_MAX))
    return -1;
  return static_cast<long>(stream->offset);
}

enum PlayerBackend { BackendNone, BackendGme, BackendVorbis };
enum PlaybackMode { PlaybackLoopAll, PlaybackLoopOne, PlaybackShuffle };

class ChiptunePlayer {
public:
  explicit ChiptunePlayer(const std::vector<std::string> &files)
      : files_(files), file_index_(0), backend_(BackendNone), emulator_(NULL),
        info_(NULL), vorbis_open_(false), vorbis_channels_(0), track_index_(0),
        track_count_(0), paused_(false), length_ms_(-1),
        playback_mode_(PlaybackLoopAll),
        random_state_(0x9e3779b9U ^ static_cast<uint32_t>(getpid()) ^
                      static_cast<uint32_t>(files.size())) {
    std::memset(&vorbis_, 0, sizeof(vorbis_));
  }
  ~ChiptunePlayer() { close_track(); }

  bool open_first(std::string *error) {
    if (files_.empty())
      return false;
    for (size_t attempts = 0; attempts < files_.size(); ++attempts) {
      if (open_file(file_index_, 0, error))
        return true;
      file_index_ = (file_index_ + 1) % files_.size();
    }
    return false;
  }

  bool change_file(int direction, std::string *error) {
    if (files_.empty())
      return false;
    const size_t count = files_.size();
    const size_t current = file_index_;
    for (size_t attempt = 1; attempt <= count; ++attempt) {
      const size_t candidate =
          direction < 0 ? (current + count - attempt % count) % count
                        : (current + attempt) % count;
      if (open_file(candidate, 0, error))
        return true;
    }
    return false;
  }

  bool change_track(int direction, std::string *error) {
    if (backend_ != BackendGme || !emulator_ || track_count_ <= 0)
      return false;
    const int next = direction < 0
                         ? (track_index_ + track_count_ - 1) % track_count_
                         : (track_index_ + 1) % track_count_;
    return start_track(next, error);
  }

  bool generate(DeckAudio *audio, std::string *error) {
    if (backend_ == BackendNone || paused_)
      return true;
    if (backend_ == BackendVorbis)
      return generate_vorbis(audio, error);
    samples_.assign(kFramesPerTick * 2, 0);
    const gme_err_t result =
        gme_play(emulator_, static_cast<int>(samples_.size()), &samples_[0]);
    if (result) {
      if (error)
        *error = result;
      return false;
    }
    visual_ = samples_;
    if (audio && audio->available())
      audio->write_stereo(&samples_[0], kFramesPerTick);
    if (gme_track_ended(emulator_))
      return advance_after_end(error);
    return true;
  }

  void toggle_pause() { paused_ = !paused_; }
  void cycle_playback_mode() {
    if (playback_mode_ == PlaybackLoopAll)
      playback_mode_ = PlaybackLoopOne;
    else if (playback_mode_ == PlaybackLoopOne)
      playback_mode_ = PlaybackShuffle;
    else
      playback_mode_ = PlaybackLoopAll;
  }
  PlaybackMode playback_mode() const { return playback_mode_; }
  bool ready() const { return backend_ != BackendNone; }
  bool paused() const { return paused_; }
  int position_ms() const {
    if (backend_ == BackendGme && emulator_)
      return gme_tell(emulator_);
    if (backend_ == BackendVorbis && vorbis_open_) {
      const double seconds = ov_time_tell(const_cast<OggVorbis_File *>(&vorbis_));
      return seconds >= 0.0 ? static_cast<int>(seconds * 1000.0) : 0;
    }
    return 0;
  }
  int length_ms() const { return length_ms_; }
  int track_index() const { return track_index_; }
  int track_count() const { return track_count_; }
  size_t file_index() const { return file_index_; }
  size_t file_count() const { return files_.size(); }
  const std::vector<int16_t> &visual() const { return visual_; }

  std::string title() const {
    if (backend_ == BackendVorbis && !vorbis_title_.empty())
      return vorbis_title_;
    if (backend_ == BackendGme && info_ && info_->song && info_->song[0])
      return info_->song;
    return files_.empty() ? std::string() : base_name(files_[file_index_]);
  }
  std::string subtitle() const {
    if (backend_ == BackendVorbis)
      return vorbis_artist_;
    std::string result;
    if (backend_ == BackendGme && info_ && info_->game && info_->game[0])
      result = info_->game;
    if (backend_ == BackendGme && info_ && info_->author && info_->author[0]) {
      if (!result.empty())
        result += " - ";
      result += info_->author;
    }
    return result;
  }
  std::string system() const {
    if (backend_ == BackendVorbis)
      return "OGG VORBIS";
    return info_ && info_->system ? info_->system : std::string();
  }

private:
  bool open_file(size_t index, int track, std::string *error) {
    close_track();
    if (index >= files_.size())
      return false;
    if (lower_extension(files_[index]) == ".ogg")
      return open_vorbis(index, error);
    std::vector<unsigned char> bytes;
    if (!read_chiptune(files_[index], &bytes, error))
      return false;
    Music_Emu *candidate = NULL;
    const gme_err_t result = gme_open_data(
        &bytes[0], static_cast<long>(bytes.size()), &candidate, kSampleRate);
    if (result || !candidate) {
      if (error)
        *error = result ? result : "cannot create emulator";
      return false;
    }
    emulator_ = candidate;
    backend_ = BackendGme;
    file_index_ = index;
    track_count_ = std::max(1, gme_track_count(emulator_));
    return start_track(std::max(0, std::min(track_count_ - 1, track)), error);
  }

  bool open_vorbis(size_t index, std::string *error) {
    if (!read_chiptune(files_[index], &file_bytes_, error))
      return false;
    memory_stream_.bytes = &file_bytes_;
    memory_stream_.offset = 0;
    const ov_callbacks callbacks = {vorbis_memory_read, vorbis_memory_seek,
                                    vorbis_memory_close, vorbis_memory_tell};
    const int result =
        ov_open_callbacks(&memory_stream_, &vorbis_, NULL, 0, callbacks);
    if (result != 0) {
      file_bytes_.clear();
      memory_stream_.bytes = NULL;
      if (error)
        *error = "cannot decode Ogg Vorbis file";
      return false;
    }
    vorbis_open_ = true;
    vorbis_info *stream_info = ov_info(&vorbis_, -1);
    if (!stream_info || stream_info->rate != static_cast<long>(kSampleRate) ||
        (stream_info->channels != 1 && stream_info->channels != 2)) {
      if (error)
        *error = "Ogg Vorbis file must be 44.1 kHz mono or stereo";
      ov_clear(&vorbis_);
      vorbis_open_ = false;
      file_bytes_.clear();
      memory_stream_.bytes = NULL;
      return false;
    }
    const double duration = ov_time_total(&vorbis_, -1);
    vorbis_comment *comments = ov_comment(&vorbis_, -1);
    const char *tag = comments ? vorbis_comment_query(comments, "TITLE", 0)
                               : NULL;
    vorbis_title_ = tag ? tag : std::string();
    tag = comments ? vorbis_comment_query(comments, "ARTIST", 0) : NULL;
    vorbis_artist_ = tag ? tag : std::string();
    backend_ = BackendVorbis;
    vorbis_channels_ = stream_info->channels;
    file_index_ = index;
    track_index_ = 0;
    track_count_ = 1;
    length_ms_ = duration >= 0.0 ? static_cast<int>(duration * 1000.0) : -1;
    paused_ = false;
    visual_.clear();
    return true;
  }

  bool generate_vorbis(DeckAudio *audio, std::string *error) {
    samples_.assign(kFramesPerTick * 2, 0);
    mono_samples_.assign(kFramesPerTick, 0);
    size_t frames = 0;
    bool rewound = false;
    while (frames < kFramesPerTick) {
      int bitstream = 0;
      const size_t remaining = kFramesPerTick - frames;
      char *destination = vorbis_channels_ == 2
                              ? reinterpret_cast<char *>(&samples_[frames * 2])
                              : reinterpret_cast<char *>(&mono_samples_[frames]);
      const int frame_bytes = vorbis_channels_ * sizeof(int16_t);
      const long amount = ov_read(&vorbis_, destination,
                                  static_cast<int>(remaining * frame_bytes),
                                  0, 2, 1, &bitstream);
      if (amount > 0) {
        const size_t decoded = static_cast<size_t>(amount) / frame_bytes;
        if (vorbis_channels_ == 1) {
          for (size_t index = 0; index < decoded; ++index) {
            const int16_t sample = mono_samples_[frames + index];
            samples_[(frames + index) * 2] = sample;
            samples_[(frames + index) * 2 + 1] = sample;
          }
        }
        frames += decoded;
        continue;
      }
      if (amount < 0) {
        if (error)
          *error = "Ogg Vorbis stream is damaged";
        return false;
      }
      if (playback_mode_ != PlaybackLoopOne) {
        visual_ = samples_;
        if (audio && audio->available())
          audio->write_stereo(&samples_[0], kFramesPerTick);
        return advance_after_end(error);
      }
      if (rewound || ov_time_seek(&vorbis_, 0.0) != 0) {
        if (error)
          *error = "Ogg Vorbis stream contains no audio";
        return false;
      }
      rewound = true;
    }
    visual_ = samples_;
    if (audio && audio->available())
      audio->write_stereo(&samples_[0], kFramesPerTick);
    return true;
  }

  uint32_t next_random() {
    if (random_state_ == 0)
      random_state_ = 0x6d2b79f5U;
    random_state_ ^= random_state_ << 13;
    random_state_ ^= random_state_ >> 17;
    random_state_ ^= random_state_ << 5;
    return random_state_;
  }

  bool change_random_file(std::string *error) {
    if (files_.empty())
      return false;
    const size_t count = files_.size();
    const size_t current = file_index_;
    const size_t offset = count > 1 ? 1 + next_random() % (count - 1) : 0;
    for (size_t attempt = 0; attempt < count; ++attempt) {
      const size_t candidate = (current + offset + attempt) % count;
      if (count > 1 && candidate == current)
        continue;
      if (open_file(candidate, 0, error)) {
        if (backend_ == BackendGme && track_count_ > 1)
          return start_track(static_cast<int>(next_random() % track_count_),
                             error);
        return true;
      }
    }
    if (!open_file(current, 0, error))
      return false;
    if (backend_ == BackendGme && track_count_ > 1)
      return start_track(static_cast<int>(next_random() % track_count_), error);
    return true;
  }

  bool advance_after_end(std::string *error) {
    if (playback_mode_ == PlaybackLoopOne) {
      if (backend_ == BackendGme)
        return start_track(track_index_, error);
      if (backend_ == BackendVorbis && ov_time_seek(&vorbis_, 0.0) == 0)
        return true;
      if (error)
        *error = "cannot restart track";
      return false;
    }
    if (playback_mode_ == PlaybackShuffle)
      return change_random_file(error);
    if (backend_ == BackendGme && track_index_ + 1 < track_count_)
      return start_track(track_index_ + 1, error);
    return change_file(1, error);
  }

  bool start_track(int track, std::string *error) {
    if (!emulator_ || track < 0 || track >= track_count_)
      return false;
    const gme_err_t result = gme_start_track(emulator_, track);
    if (result) {
      if (error)
        *error = result;
      return false;
    }
    if (info_)
      gme_free_info(info_);
    info_ = NULL;
    const gme_err_t info_result = gme_track_info(emulator_, &info_, track);
    if (info_result)
      info_ = NULL;
    track_index_ = track;
    length_ms_ = info_ ? info_->play_length : -1;
    paused_ = false;
    visual_.clear();
    return true;
  }

  void close_track() {
    if (info_)
      gme_free_info(info_);
    info_ = NULL;
    if (emulator_)
      gme_delete(emulator_);
    emulator_ = NULL;
    if (vorbis_open_)
      ov_clear(&vorbis_);
    std::memset(&vorbis_, 0, sizeof(vorbis_));
    vorbis_open_ = false;
    vorbis_channels_ = 0;
    file_bytes_.clear();
    vorbis_title_.clear();
    vorbis_artist_.clear();
    memory_stream_.bytes = NULL;
    memory_stream_.offset = 0;
    backend_ = BackendNone;
    track_count_ = 0;
    track_index_ = 0;
    length_ms_ = -1;
    visual_.clear();
  }

  std::vector<std::string> files_;
  size_t file_index_;
  PlayerBackend backend_;
  Music_Emu *emulator_;
  gme_info_t *info_;
  OggVorbis_File vorbis_;
  bool vorbis_open_;
  int vorbis_channels_;
  std::vector<unsigned char> file_bytes_;
  VorbisMemoryStream memory_stream_;
  std::string vorbis_title_;
  std::string vorbis_artist_;
  int track_index_;
  int track_count_;
  bool paused_;
  int length_ms_;
  PlaybackMode playback_mode_;
  uint32_t random_state_;
  std::vector<int16_t> samples_;
  std::vector<int16_t> mono_samples_;
  std::vector<int16_t> visual_;
};

std::string format_time(int milliseconds) {
  const int seconds = std::max(0, milliseconds / 1000);
  char text[32];
  std::snprintf(text, sizeof(text), "%d:%02d", seconds / 60, seconds % 60);
  return text;
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
                      uint16_t border, int thickness = 2) {
  fill_pixel_cut_rect(canvas, rect, thickness, border);
  fill_pixel_cut_rect(canvas,
                      Rect{rect.x + thickness, rect.y + thickness,
                           rect.width - thickness * 2,
                           rect.height - thickness * 2},
                      thickness, fill);
}

void draw_outline_arrow(Canvas *canvas, const Rect &bounds, bool points_left,
                        uint16_t color) {
  const int center_x = bounds.x + bounds.width / 2;
  const int center_y = bounds.y + bounds.height / 2;
  const int mirror = points_left ? -1 : 1;
  const auto block = [&](int x, int y, int width, int height) {
    const int left = mirror < 0 ? center_x - x - width : center_x + x;
    fill_rect(canvas, Rect{left, center_y + y, width, height}, color);
  };
  block(14, -1, 2, 2);
  block(12, -3, 2, 2);
  block(10, -5, 2, 2);
  block(8, -7, 2, 2);
  block(6, -9, 2, 2);
  block(4, -11, 2, 5);
  block(-14, -6, 18, 2);
  block(-14, -4, 2, 8);
  block(-14, 4, 18, 2);
  block(4, 6, 2, 5);
  block(6, 7, 2, 2);
  block(8, 5, 2, 2);
  block(10, 3, 2, 2);
  block(12, 1, 2, 2);
}

void draw_close_icon(Canvas *canvas, const Rect &bounds, uint16_t color) {
  const int center_x = bounds.x + bounds.width / 2;
  const int center_y = bounds.y + bounds.height / 2;
  for (int offset = -8; offset <= 8; offset += 2) {
    fill_rect(canvas,
              Rect{center_x + offset, center_y + offset, 2, 2}, color);
    fill_rect(canvas,
              Rect{center_x + offset, center_y - offset, 2, 2}, color);
  }
}

void draw_pixel_line(Canvas *canvas, int from_x, int from_y, int to_x,
                     int to_y, int thickness, uint16_t color) {
  const int delta_x = std::abs(to_x - from_x);
  const int step_x = from_x < to_x ? 1 : -1;
  const int delta_y = -std::abs(to_y - from_y);
  const int step_y = from_y < to_y ? 1 : -1;
  int error = delta_x + delta_y;
  while (true) {
    fill_rect(canvas, Rect{from_x, from_y, thickness, thickness}, color);
    if (from_x == to_x && from_y == to_y)
      break;
    const int doubled_error = error * 2;
    if (doubled_error >= delta_y) {
      error += delta_y;
      from_x += step_x;
    }
    if (doubled_error <= delta_x) {
      error += delta_x;
      from_y += step_y;
    }
  }
}

void draw_arrow_head(Canvas *canvas, int point_x, int point_y,
                     bool points_right, uint16_t color) {
  const int direction = points_right ? -1 : 1;
  draw_pixel_line(canvas, point_x, point_y, point_x + direction * 4,
                  point_y - 4, 1, color);
  draw_pixel_line(canvas, point_x, point_y, point_x + direction * 4,
                  point_y + 4, 1, color);
}

void draw_transport_triangle(Canvas *canvas, int center_x, int center_y,
                             bool points_right, uint16_t color) {
  for (int row = -6; row <= 6; row += 2) {
    const int width = 14 - std::abs(row) * 2;
    const int left = points_right ? center_x - 6 : center_x + 6 - width;
    fill_rect(canvas, Rect{left, center_y + row, width, 2}, color);
  }
}

void draw_previous_icon(Canvas *canvas, const Rect &rect, uint16_t color) {
  const int center_x = rect.x + rect.width / 2;
  const int center_y = rect.y + rect.height / 2;
  fill_rect(canvas, Rect{center_x - 10, center_y - 7, 2, 14}, color);
  draw_transport_triangle(canvas, center_x + 1, center_y, false, color);
}

void draw_next_icon(Canvas *canvas, const Rect &rect, uint16_t color) {
  const int center_x = rect.x + rect.width / 2;
  const int center_y = rect.y + rect.height / 2;
  draw_transport_triangle(canvas, center_x - 1, center_y, true, color);
  fill_rect(canvas, Rect{center_x + 8, center_y - 7, 2, 14}, color);
}

void draw_pause_icon(Canvas *canvas, const Rect &rect, bool paused,
                     uint16_t color) {
  const int center_x = rect.x + rect.width / 2;
  const int center_y = rect.y + rect.height / 2;
  if (paused) {
    draw_transport_triangle(canvas, center_x - 1, center_y, true, color);
    return;
  }
  fill_rect(canvas, Rect{center_x - 5, center_y - 7, 3, 14}, color);
  fill_rect(canvas, Rect{center_x + 2, center_y - 7, 3, 14}, color);
}

void draw_loop_icon(Canvas *canvas, const Rect &rect, bool one,
                    uint16_t color) {
  const int center_x = rect.x + rect.width / 2;
  const int center_y = rect.y + rect.height / 2;
  draw_pixel_line(canvas, center_x - 11, center_y - 5, center_x + 9,
                  center_y - 5, 1, color);
  draw_arrow_head(canvas, center_x + 11, center_y - 5, true, color);
  draw_pixel_line(canvas, center_x + 11, center_y + 5, center_x - 9,
                  center_y + 5, 1, color);
  draw_arrow_head(canvas, center_x - 11, center_y + 5, false, color);
  if (one)
    draw_text(canvas, center_x - 2, center_y - 3, "1", 1, color);
}

void draw_shuffle_icon(Canvas *canvas, const Rect &rect, uint16_t color) {
  const int center_x = rect.x + rect.width / 2;
  const int center_y = rect.y + rect.height / 2;
  draw_pixel_line(canvas, center_x - 12, center_y - 5, center_x - 7,
                  center_y - 5, 1, color);
  draw_pixel_line(canvas, center_x - 7, center_y - 5, center_x + 6,
                  center_y + 5, 1, color);
  draw_pixel_line(canvas, center_x - 12, center_y + 5, center_x - 7,
                  center_y + 5, 1, color);
  draw_pixel_line(canvas, center_x - 7, center_y + 5, center_x + 6,
                  center_y - 5, 1, color);
  draw_pixel_line(canvas, center_x + 6, center_y - 5, center_x + 10,
                  center_y - 5, 1, color);
  draw_pixel_line(canvas, center_x + 6, center_y + 5, center_x + 10,
                  center_y + 5, 1, color);
  draw_arrow_head(canvas, center_x + 12, center_y - 5, true, color);
  draw_arrow_head(canvas, center_x + 12, center_y + 5, true, color);
}

void draw_playback_mode_icon(Canvas *canvas, const Rect &rect,
                             PlaybackMode mode, uint16_t color) {
  if (mode == PlaybackShuffle) {
    draw_shuffle_icon(canvas, rect, color);
    return;
  }
  draw_loop_icon(canvas, rect, mode == PlaybackLoopOne, color);
}

void draw_file_indicators(Canvas *canvas, size_t file_index, size_t file_count,
                          uint16_t orange, uint16_t inactive) {
  if (file_count == 0)
    return;
  const size_t visible = std::min<size_t>(file_count, 40);
  const int width = 6;
  const int gap = 4;
  const int row_width = static_cast<int>(visible) * width +
                        static_cast<int>(visible - 1) * gap;
  int x = (kCanvasWidth - row_width) / 2;
  size_t first = 0;
  if (file_count > visible) {
    first = file_index > visible / 2 ? file_index - visible / 2 : 0;
    first = std::min(first, file_count - visible);
  }
  for (size_t index = 0; index < visible; ++index) {
    const bool selected = first + index == file_index;
    stroke_rect(canvas, Rect{x, 166, width, 4}, 1,
                selected ? orange : inactive);
    x += width + gap;
  }
}

void render_player(Canvas *canvas, const ChiptunePlayer &player,
                   const std::string &status, unsigned int volume_percent) {
  const uint16_t background = DeckRgb888To565(0x000000);
  const uint16_t orange = DeckRgb888To565(0xfe6c27);
  const uint16_t active = DeckRgb888To565(0x4d372d);
  const uint16_t text = DeckRgb888To565(0xffffff);
  const uint16_t green = DeckRgb888To565(0x87af87);
  const uint16_t red = DeckRgb888To565(0xaf8787);
  const uint16_t muted = DeckRgb888To565(0x969696);
  const uint16_t indicator = DeckRgb888To565(0x6c6c6c);
  canvas->assign(static_cast<size_t>(kCanvasWidth * kCanvasHeight), background);

  draw_pixel_panel(canvas, Rect{236, 4, 152, 29}, active, orange);
  draw_centered_text(canvas, 12, "CHIPTUNES", 1, text);
  draw_close_icon(canvas, kCloseButton, text);
  char volume[24];
  if (volume_percent)
    std::snprintf(volume, sizeof(volume), "VOL %u", volume_percent);
  else
    std::snprintf(volume, sizeof(volume), "VOL OFF");
  draw_text(canvas, 8, 14, volume, 1, volume_percent ? green : red);

  if (!player.ready()) {
    draw_pixel_panel(canvas, Rect{78, 42, 468, 120}, active, orange);
    draw_centered_text(canvas, 72, "NO CHIPTUNES FOUND", 2, text);
    draw_centered_text(canvas, 103, clipped(status, 72), 1, muted);
    draw_centered_text(
        canvas, 126,
        "AY GBS GYM HES KSS NSF NSFE OGG SAP SPC VGM VGZ", 1, text);
  } else {
    draw_pixel_panel(canvas, Rect{78, 42, 468, 120}, active, orange);
    draw_centered_text(canvas, 50, clipped(player.title(), 45), 2, text);
    draw_centered_text(canvas, 70, clipped(player.subtitle(), 72), 1, muted);

    const std::vector<int16_t> &samples = player.visual();
    fill_rect(canvas, Rect{96, 84, 432, 44}, background);
    fill_rect(canvas, Rect{96, 105, 432, 1}, muted);
    if (!samples.empty()) {
      for (int x = 0; x < 432; ++x) {
        const size_t frame = static_cast<size_t>(x) * kFramesPerTick / 432;
        const int mixed = (static_cast<int>(samples[frame * 2]) +
                           static_cast<int>(samples[frame * 2 + 1])) /
                          2;
        const int height = std::min(20, std::abs(mixed) / 1050);
        fill_rect(canvas, Rect{96 + x, mixed < 0 ? 106 : 105 - height, 1,
                               std::max(1, height)},
                  orange);
      }
    }

    const int position = player.position_ms();
    const int length = player.length_ms();
    const int progress =
        length > 0 ? std::max(0, std::min(432, position * 432 / length)) : 0;
    fill_rect(canvas, Rect{96, 134, 432, 3}, background);
    if (progress > 0)
      fill_rect(canvas, Rect{96, 134, progress, 3}, green);
    draw_text(canvas, 96, 143, format_time(position), 1, text);
    const std::string end_time = length > 0 ? format_time(length) : "--:--";
    draw_text(canvas, 528 - static_cast<int>(end_time.size() * 6 - 1), 143,
              end_time, 1, text);

    char details[128];
    std::snprintf(details, sizeof(details), "%s  FILE %zu/%zu  TRACK %d/%d",
                  clipped(player.system(), 18).c_str(), player.file_index() + 1,
                  player.file_count(), player.track_index() + 1,
                  player.track_count());
    draw_centered_text(canvas, 143, clipped(details, 56), 1, muted);
    draw_file_indicators(canvas, player.file_index(), player.file_count(),
                         orange, indicator);
  }

  draw_outline_arrow(canvas, kPreviousFileButton, true, orange);
  draw_outline_arrow(canvas, kNextFileButton, false, orange);
  draw_playback_mode_icon(canvas, kPlaybackModeButton, player.playback_mode(),
                          text);
  draw_previous_icon(canvas, kPreviousTrackButton, text);
  draw_pause_icon(canvas, kPauseButton, player.paused(), text);
  draw_next_icon(canvas, kNextTrackButton, text);
}

int render_preview(const char *track_path, const char *output_path) {
  std::vector<std::string> paths(1, track_path);
  ChiptunePlayer player(paths);
  std::string error;
  if (!player.open_first(&error)) {
    std::fprintf(stderr, "chiptune-deck: preview open failed: %s\n",
                 error.c_str());
    return 1;
  }
  for (int block = 0; block < 4; ++block) {
    if (!player.generate(NULL, &error)) {
      std::fprintf(stderr, "chiptune-deck: preview playback failed: %s\n",
                   error.c_str());
      return 1;
    }
  }
  Canvas canvas;
  render_player(&canvas, player, std::string(), 42);
  FILE *output = std::fopen(output_path, "wb");
  if (!output) {
    std::fprintf(stderr, "chiptune-deck: cannot write preview: %s\n",
                 std::strerror(errno));
    return 1;
  }
  std::fprintf(output, "P6\n%d %d\n255\n", kCanvasWidth, kCanvasHeight);
  for (size_t index = 0; index < canvas.size(); ++index) {
    const uint16_t pixel = canvas[index];
    const unsigned char rgb[3] = {
        static_cast<unsigned char>(((pixel >> 11) & 0x1f) * 255 / 31),
        static_cast<unsigned char>(((pixel >> 5) & 0x3f) * 255 / 63),
        static_cast<unsigned char>((pixel & 0x1f) * 255 / 31)};
    if (std::fwrite(rgb, sizeof(rgb), 1, output) != 1) {
      std::fprintf(stderr, "chiptune-deck: cannot finish preview\n");
      std::fclose(output);
      return 1;
    }
  }
  if (std::fclose(output) != 0) {
    std::fprintf(stderr, "chiptune-deck: cannot close preview: %s\n",
                 std::strerror(errno));
    return 1;
  }
  return 0;
}

int probe_file(const char *path) {
  if (lower_extension(path) == ".ogg") {
    std::vector<std::string> paths(1, path);
    ChiptunePlayer player(paths);
    std::string error;
    if (!player.open_first(&error)) {
      std::fprintf(stderr, "chiptune-deck: probe open failed: %s\n",
                   error.c_str());
      return 1;
    }
    int peak = 0;
    for (int block = 0; block < 60; ++block) {
      if (!player.generate(NULL, &error)) {
        std::fprintf(stderr, "chiptune-deck: probe playback failed: %s\n",
                     error.c_str());
        return 1;
      }
      const std::vector<int16_t> &samples = player.visual();
      for (size_t index = 0; index < samples.size(); ++index)
        peak = std::max(peak, std::abs(static_cast<int>(samples[index])));
    }
    std::printf("tracks=1 samples=%zu peak=%d\n",
                static_cast<size_t>(60) * kFramesPerTick * 2, peak);
    return peak > 0 ? 0 : 1;
  }
  std::vector<unsigned char> bytes;
  std::string read_error;
  if (!read_chiptune(path, &bytes, &read_error)) {
    std::fprintf(stderr, "chiptune-deck: probe read failed: %s\n",
                 read_error.c_str());
    return 1;
  }
  Music_Emu *emulator = NULL;
  const gme_err_t open_error = gme_open_data(
      &bytes[0], static_cast<long>(bytes.size()), &emulator, kSampleRate);
  if (open_error || !emulator) {
    std::fprintf(stderr, "chiptune-deck: probe open failed: %s\n",
                 open_error ? open_error : "cannot create emulator");
    return 1;
  }
  const int tracks = gme_track_count(emulator);
  const gme_err_t track_error = gme_start_track(emulator, 0);
  std::vector<int16_t> samples(kSampleRate * 2, 0);
  const gme_err_t play_error =
      track_error ? track_error
                  : gme_play(emulator, static_cast<int>(samples.size()),
                             &samples[0]);
  int peak = 0;
  for (size_t index = 0; index < samples.size(); ++index)
    peak = std::max(peak, std::abs(static_cast<int>(samples[index])));
  gme_delete(emulator);
  if (play_error) {
    std::fprintf(stderr, "chiptune-deck: probe playback failed: %s\n",
                 play_error);
    return 1;
  }
  std::printf("tracks=%d samples=%zu peak=%d\n", tracks, samples.size(), peak);
  return tracks > 0 ? 0 : 1;
}

} // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, NULL, _IOLBF, 0);
  std::setvbuf(stderr, NULL, _IOLBF, 0);
  if (argc == 3 && std::string(argv[1]) == "--probe")
    return probe_file(argv[2]);
  if (argc == 4 && std::string(argv[1]) == "--render-preview")
    return render_preview(argv[2], argv[3]);
  if (argc != 2) {
    std::fprintf(stderr,
                 "Usage: %s CHIPTUNE_DIRECTORY\n"
                 "       %s --probe CHIPTUNE_FILE\n"
                 "       %s --render-preview CHIPTUNE_FILE OUTPUT.ppm\n",
                 argv[0], argv[0], argv[0]);
    return 2;
  }
  install_signal_handlers();

  std::vector<std::string> files;
  scan_chiptunes(argv[1], 0, &files);
  std::sort(files.begin(), files.end());
  std::fprintf(stderr, "chiptune-deck: found %zu supported file(s) in %s\n",
               files.size(), argv[1]);

  std::string error;
  DeckFramebuffer framebuffer;
  if (!framebuffer.open_device(&error)) {
    std::fprintf(stderr, "chiptune-deck: %s\n", error.c_str());
    return 1;
  }
  PlayerInput input;
  input.discover();

  unsigned int volume_percent = 0;
  if (!DeckReadVolumePercent(&volume_percent, &error)) {
    std::fprintf(stderr, "chiptune-deck: %s; audio muted\n", error.c_str());
    volume_percent = 0;
  }
  const char *volume_state_environment =
      std::getenv("RETRO_DECK_VOLUME_STATE");
  const std::string volume_state =
      volume_state_environment ? volume_state_environment : std::string();
  DeckAudio audio;
  if (!audio.open_device(kSampleRate, volume_percent, &error)) {
    std::fprintf(stderr, "chiptune-deck: %s; continuing muted\n",
                 error.c_str());
  }

  ChiptunePlayer player(files);
  std::string status = std::string("ADD MUSIC TO ") + argv[1];
  if (!files.empty() && !player.open_first(&error)) {
    status = std::string("CANNOT PLAY FILES: ") + error;
    std::fprintf(stderr, "chiptune-deck: %s\n", status.c_str());
  }

  Canvas canvas;
  DeckFrameClock clock(60.0);
  unsigned int frame_number = 0;
  bool dirty = true;
  while (!shutdown_requested) {
    const unsigned int commands = input.read_commands();
    if (commands & ControlBack)
      break;
    if ((commands & ControlPreviousFile) && player.change_file(-1, &error))
      dirty = true;
    if ((commands & ControlNextFile) && player.change_file(1, &error))
      dirty = true;
    if ((commands & ControlPreviousTrack) && player.change_track(-1, &error))
      dirty = true;
    if ((commands & ControlNextTrack) && player.change_track(1, &error))
      dirty = true;
    if (commands & ControlTogglePause) {
      player.toggle_pause();
      dirty = true;
    }
    if (commands & ControlCyclePlaybackMode) {
      player.cycle_playback_mode();
      dirty = true;
    }
    if (commands & (ControlVolumeDown | ControlVolumeUp)) {
      const unsigned int requested =
          commands & ControlVolumeUp
              ? std::min(100u, volume_percent + 5u)
              : (volume_percent >= 5 ? volume_percent - 5 : 0);
      if (requested != volume_percent) {
        std::string volume_error;
        if (audio.open_device(kSampleRate, requested, &volume_error)) {
          volume_percent = requested;
          dirty = true;
          if (!save_player_volume(volume_state, volume_percent,
                                  &volume_error)) {
            std::fprintf(stderr, "chiptune-deck: %s\n",
                         volume_error.c_str());
          }
        } else {
          std::fprintf(stderr, "chiptune-deck: cannot change volume: %s\n",
                       volume_error.c_str());
        }
      }
    }

    if (!player.generate(&audio, &error)) {
      status = std::string("PLAYBACK ERROR: ") + error;
      std::fprintf(stderr, "chiptune-deck: %s\n", status.c_str());
    }
    if (dirty || frame_number % 2 == 0) {
      render_player(&canvas, player, status, volume_percent);
      if (!framebuffer.present_rgb565(&canvas[0], kCanvasWidth, kCanvasHeight,
                                      kCanvasWidth * sizeof(canvas[0]),
                                      &error)) {
        std::fprintf(stderr, "chiptune-deck: %s\n", error.c_str());
        return 1;
      }
      dirty = false;
    }
    ++frame_number;
    clock.wait_for_next_frame();
  }
  return 0;
}
