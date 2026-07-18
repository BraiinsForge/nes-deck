#include "menu_io.h"

#include <cerrno>
#include <cstring>
#include <unistd.h>

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
