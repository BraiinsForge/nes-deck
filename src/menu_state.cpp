#include "menu_state.h"

#include "menu_io.h"
#include "menu_text.h"

#include <algorithm>
#include <cerrno>
#include <climits>
#include <cstdio>
#include <cstdint>
#include <fcntl.h>
#include <sstream>
#include <sys/stat.h>
#include <unistd.h>

namespace {

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

} // namespace

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

namespace {

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

} // namespace

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
