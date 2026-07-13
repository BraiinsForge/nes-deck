#include "deck_runtime.h"

#include <algorithm>
#include <cerrno>
#include <climits>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <dirent.h>
#include <fcntl.h>
#include <iostream>
#include <linux/input.h>
#include <poll.h>
#include <string>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>
#include <vector>

namespace {

const int kLogicalWidth = 1280;
const int kLogicalHeight = 480;
const int kCanvasWidth = 624;
const int kCanvasHeight = 224;
const int64_t kCentisecondNanoseconds = 10000000;
const int64_t kRedrawNanoseconds = 16000000;

volatile sig_atomic_t shutdown_requested = 0;

struct Rect {
  int x;
  int y;
  int width;
  int height;
};

typedef std::vector<uint16_t> Canvas;

int64_t monotonic_nanoseconds() {
  struct timespec now;
  if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
    return 0;
  return static_cast<int64_t>(now.tv_sec) * 1000000000LL + now.tv_nsec;
}

std::string TenSecondsFormat(unsigned int centiseconds) {
  if (centiseconds > 9999)
    centiseconds = 9999;
  char output[6];
  std::snprintf(output, sizeof(output), "%02u.%02u", centiseconds / 100,
                centiseconds % 100);
  return output;
}

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
  return unknown;
}

void draw_text(Canvas *canvas, int x, int y, const std::string &text,
               int scale, uint16_t color) {
  for (size_t index = 0; index < text.size(); ++index) {
    const uint8_t *rows = glyph_rows(text[index]);
    for (int row = 0; row < 7; ++row) {
      for (int column = 0; column < 5; ++column) {
        if (rows[row] & (1u << (4 - column)))
          fill_rect(canvas,
                    Rect{x + static_cast<int>(index) * 6 * scale +
                             column * scale,
                         y + row * scale, scale, scale},
                    color);
      }
    }
  }
}

void draw_centered_text(Canvas *canvas, int y, const std::string &text,
                        int scale, uint16_t color) {
  const int width = text.empty() ? 0 :
      static_cast<int>(text.size() * 6 - 1) * scale;
  draw_text(canvas, std::max(0, (kCanvasWidth - width) / 2), y, text, scale,
            color);
}

void draw_digit(Canvas *canvas, int x, int y, char character,
                uint16_t active, uint16_t inactive) {
  static const unsigned int segments[10] = {
      0x3f, 0x06, 0x5b, 0x4f, 0x66, 0x6d, 0x7d, 0x07, 0x7f, 0x6f};
  const int width = 76;
  const int height = 128;
  const int thickness = 11;
  const Rect bounds[7] = {
      Rect{x + thickness, y, width - 2 * thickness, thickness},
      Rect{x + width - thickness, y + thickness, thickness,
           height / 2 - thickness},
      Rect{x + width - thickness, y + height / 2, thickness,
           height / 2 - thickness},
      Rect{x + thickness, y + height - thickness, width - 2 * thickness,
           thickness},
      Rect{x, y + height / 2, thickness, height / 2 - thickness},
      Rect{x, y + thickness, thickness, height / 2 - thickness},
      Rect{x + thickness, y + height / 2 - thickness / 2,
           width - 2 * thickness, thickness}};
  const unsigned int mask = character >= '0' && character <= '9'
                                ? segments[character - '0']
                                : 0;
  for (int index = 0; index < 7; ++index)
    fill_rect(canvas, bounds[index], mask & (1u << index) ? active : inactive);
}

enum TimerState { TimerReady, TimerRunning, TimerStopped };

void render_timer(Canvas *canvas, TimerState state,
                  unsigned int displayed_centiseconds) {
  const uint16_t background = DeckRgb888To565(0x100d0c);
  const uint16_t amber = DeckRgb888To565(0xff7138);
  const uint16_t dim_amber = DeckRgb888To565(0x351b15);
  const uint16_t cream = DeckRgb888To565(0xffedc2);
  const uint16_t muted = DeckRgb888To565(0xaa8f7c);
  const uint16_t success = DeckRgb888To565(0x62d38c);
  const uint16_t button = DeckRgb888To565(0x29211e);
  canvas->assign(static_cast<size_t>(kCanvasWidth * kCanvasHeight), background);

  fill_rect(canvas, Rect{6, 5, 70, 25}, button);
  draw_text(canvas, 15, 11, "BACK", 2, cream);
  draw_centered_text(canvas, 9, "STOP AT 10.00", 2, cream);

  const std::string shown = TenSecondsFormat(displayed_centiseconds);
  const uint16_t digit_color =
      state == TimerStopped && displayed_centiseconds == 1000 ? success : amber;
  const int positions[4] = {129, 219, 329, 419};
  draw_digit(canvas, positions[0], 43, shown[0], digit_color, dim_amber);
  draw_digit(canvas, positions[1], 43, shown[1], digit_color, dim_amber);
  draw_digit(canvas, positions[2], 43, shown[3], digit_color, dim_amber);
  draw_digit(canvas, positions[3], 43, shown[4], digit_color, dim_amber);
  fill_rect(canvas, Rect{303, 149, 14, 14}, digit_color);

  std::string result;
  if (state == TimerStopped) {
    if (displayed_centiseconds == 1000) {
      result = "EXACT";
    } else if (displayed_centiseconds < 1000) {
      result = TenSecondsFormat(1000 - displayed_centiseconds) + " EARLY";
    } else {
      result = TenSecondsFormat(displayed_centiseconds - 1000) + " LATE";
    }
  }
  if (!result.empty())
    draw_centered_text(canvas, 178, result, 1,
                       displayed_centiseconds == 1000 ? success : muted);

  const std::string instruction =
      state == TimerReady
          ? "TAP TO START"
          : (state == TimerRunning ? "TAP TO STOP" : "TAP FOR ANOTHER TRY");
  draw_centered_text(canvas, 198, instruction, 2, cream);
}

bool bit_is_set(const unsigned long *bits, unsigned int bit) {
  const unsigned int bits_per_word = sizeof(unsigned long) * CHAR_BIT;
  return (bits[bit / bits_per_word] & (1UL << (bit % bits_per_word))) != 0;
}

struct TouchPress {
  int x;
  int y;
};

class TouchDevice {
public:
  TouchDevice()
      : fd_(-1), x_(0), y_(0), down_(false), reported_down_(false),
        dropping_(false), grabbed_(false) {}
  ~TouchDevice() { close_device(); }

  int fd() const { return fd_; }

  bool discover(std::string *error) {
    close_device();
    DIR *directory = opendir("/dev/input");
    if (!directory) {
      if (error)
        *error = std::string("cannot open /dev/input: ") +
                 std::strerror(errno);
      return false;
    }
    std::vector<std::string> paths;
    for (struct dirent *entry = readdir(directory); entry;
         entry = readdir(directory)) {
      const std::string name(entry->d_name);
      if (name.size() <= 5 || name.compare(0, 5, "event") != 0)
        continue;
      bool numeric = true;
      for (size_t index = 5; index < name.size(); ++index)
        numeric = numeric && name[index] >= '0' && name[index] <= '9';
      if (numeric)
        paths.push_back("/dev/input/" + name);
    }
    closedir(directory);
    std::sort(paths.begin(), paths.end());

    for (size_t index = 0; index < paths.size(); ++index) {
      const int candidate =
          open(paths[index].c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
      if (candidate < 0)
        continue;
      char name[256] = {};
      struct input_absinfo x_info;
      struct input_absinfo y_info;
      std::memset(&x_info, 0, sizeof(x_info));
      std::memset(&y_info, 0, sizeof(y_info));
      if (ioctl(candidate, EVIOCGNAME(sizeof(name)), name) < 0 ||
          std::string(name).find("Goodix Capacitive TouchScreen") ==
              std::string::npos ||
          ioctl(candidate, EVIOCGABS(ABS_X), &x_info) != 0 ||
          ioctl(candidate, EVIOCGABS(ABS_Y), &y_info) != 0 ||
          x_info.minimum != 0 || x_info.maximum != kLogicalWidth - 1 ||
          y_info.minimum != 0 || y_info.maximum != kLogicalHeight - 1) {
        close(candidate);
        continue;
      }
      fd_ = candidate;
      x_ = x_info.value;
      y_ = y_info.value;
      const size_t words = (KEY_MAX + sizeof(unsigned long) * CHAR_BIT) /
                           (sizeof(unsigned long) * CHAR_BIT);
      std::vector<unsigned long> keys(words, 0);
      if (ioctl(fd_, EVIOCGKEY(keys.size() * sizeof(unsigned long)),
                &keys[0]) >= 0)
        down_ = bit_is_set(&keys[0], BTN_TOUCH);
      reported_down_ = down_;
      if (ioctl(fd_, EVIOCGRAB, 1) == 0)
        grabbed_ = true;
      else
        std::fprintf(stderr, "ten-seconds-deck: cannot grab touchscreen: %s\n",
                     std::strerror(errno));
      return true;
    }
    if (error)
      *error = "Goodix Capacitive TouchScreen was not found";
    return false;
  }

  bool read_presses(std::vector<TouchPress> *presses, std::string *error) {
    if (!presses || fd_ < 0)
      return false;
    presses->clear();
    while (true) {
      struct input_event events[32];
      const ssize_t amount = read(fd_, events, sizeof(events));
      if (amount < 0) {
        if (errno == EINTR)
          continue;
        if (errno == EAGAIN || errno == EWOULDBLOCK)
          return true;
        if (error)
          *error = std::string("touchscreen read failed: ") +
                   std::strerror(errno);
        return false;
      }
      if (amount <= 0 ||
          amount % static_cast<ssize_t>(sizeof(struct input_event)) != 0) {
        if (error)
          *error = "touchscreen disconnected or returned a partial event";
        return false;
      }
      const size_t count = static_cast<size_t>(amount) / sizeof(events[0]);
      for (size_t index = 0; index < count; ++index) {
        const struct input_event &event = events[index];
        if (event.type == EV_SYN && event.code == SYN_DROPPED) {
          dropping_ = true;
          continue;
        }
        if (dropping_) {
          if (event.type == EV_SYN && event.code == SYN_REPORT) {
            resynchronize();
            if (down_ && !reported_down_)
              presses->push_back(TouchPress{x_, y_});
            reported_down_ = down_;
            dropping_ = false;
          }
          continue;
        }
        if (event.type == EV_ABS && event.code == ABS_X)
          x_ = std::max(0, std::min(kLogicalWidth - 1, event.value));
        else if (event.type == EV_ABS && event.code == ABS_Y)
          y_ = std::max(0, std::min(kLogicalHeight - 1, event.value));
        else if (event.type == EV_KEY && event.code == BTN_TOUCH)
          down_ = event.value != 0;
        else if (event.type == EV_SYN && event.code == SYN_REPORT) {
          if (down_ && !reported_down_)
            presses->push_back(TouchPress{x_, y_});
          reported_down_ = down_;
        }
      }
    }
  }

private:
  void resynchronize() {
    struct input_absinfo info;
    if (ioctl(fd_, EVIOCGABS(ABS_X), &info) == 0)
      x_ = std::max(0, std::min(kLogicalWidth - 1, info.value));
    if (ioctl(fd_, EVIOCGABS(ABS_Y), &info) == 0)
      y_ = std::max(0, std::min(kLogicalHeight - 1, info.value));
    const size_t words = (KEY_MAX + sizeof(unsigned long) * CHAR_BIT) /
                         (sizeof(unsigned long) * CHAR_BIT);
    std::vector<unsigned long> keys(words, 0);
    if (ioctl(fd_, EVIOCGKEY(keys.size() * sizeof(unsigned long)), &keys[0]) >=
        0)
      down_ = bit_is_set(&keys[0], BTN_TOUCH);
  }

  void close_device() {
    if (fd_ >= 0) {
      if (grabbed_)
        ioctl(fd_, EVIOCGRAB, 0);
      close(fd_);
    }
    fd_ = -1;
    grabbed_ = false;
  }

  int fd_;
  int x_;
  int y_;
  bool down_;
  bool reported_down_;
  bool dropping_;
  bool grabbed_;
};

bool is_back_press(const TouchPress &press) {
  return press.x >= 16 && press.x < 168 && press.y >= 16 && press.y < 80;
}

} // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, NULL, _IOLBF, 0);
  std::setvbuf(stderr, NULL, _IOLBF, 0);
  if (argc != 1) {
    std::fprintf(stderr, "Usage: %s\n", argv[0]);
    return 2;
  }
  install_signal_handlers();

  std::string error;
  DeckFramebuffer framebuffer;
  if (!framebuffer.open_device(&error)) {
    std::fprintf(stderr, "ten-seconds-deck: %s\n", error.c_str());
    return 1;
  }
  TouchDevice touch;
  if (!touch.discover(&error)) {
    std::fprintf(stderr, "ten-seconds-deck: %s\n", error.c_str());
    return 1;
  }

  Canvas canvas;
  TimerState state = TimerReady;
  int64_t started_at = 0;
  int64_t next_redraw = 0;
  unsigned int displayed = 0;
  bool dirty = true;

  while (!shutdown_requested) {
    const int64_t now = monotonic_nanoseconds();
    if (state == TimerRunning && now >= next_redraw) {
      const int64_t elapsed = std::max<int64_t>(0, now - started_at);
      displayed = static_cast<unsigned int>(
          std::min<int64_t>(9999, elapsed / kCentisecondNanoseconds));
      next_redraw = now + kRedrawNanoseconds;
      dirty = true;
    }
    if (dirty) {
      render_timer(&canvas, state, displayed);
      if (!framebuffer.present_rgb565(&canvas[0], kCanvasWidth, kCanvasHeight,
                                      kCanvasWidth * sizeof(canvas[0]),
                                      &error)) {
        std::fprintf(stderr, "ten-seconds-deck: %s\n", error.c_str());
        return 1;
      }
      dirty = false;
    }

    struct pollfd descriptor;
    descriptor.fd = touch.fd();
    descriptor.events = POLLIN;
    descriptor.revents = 0;
    const int poll_result = poll(&descriptor, 1, 8);
    if (poll_result < 0) {
      if (errno == EINTR)
        continue;
      std::fprintf(stderr, "ten-seconds-deck: poll failed: %s\n",
                   std::strerror(errno));
      return 1;
    }
    if (poll_result == 0)
      continue;
    std::vector<TouchPress> presses;
    if (!touch.read_presses(&presses, &error)) {
      std::fprintf(stderr, "ten-seconds-deck: %s\n", error.c_str());
      return 1;
    }
    for (size_t index = 0; index < presses.size(); ++index) {
      const TouchPress &press = presses[index];
      if (is_back_press(press)) {
        shutdown_requested = 1;
        break;
      }
      const int64_t pressed_at = monotonic_nanoseconds();
      if (state == TimerRunning) {
        const int64_t elapsed = std::max<int64_t>(0, pressed_at - started_at);
        displayed = static_cast<unsigned int>(
            std::min<int64_t>(9999, elapsed / kCentisecondNanoseconds));
        std::fprintf(stderr, "ten-seconds-deck: result=%s input=touch\n",
                     TenSecondsFormat(displayed).c_str());
        state = TimerStopped;
      } else {
        started_at = pressed_at;
        next_redraw = pressed_at;
        displayed = 0;
        state = TimerRunning;
      }
      dirty = true;
    }
  }
  return 0;
}
