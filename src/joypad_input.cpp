/*===================================================================*/
/*                                                                   */
/*  joypad_input.cpp : Input handler for Braiins Forge Deck          */
/*                                                                   */
/*  Supports a raw TTY keyboard plus two Retro Games THEGamepads.    */
/*  Identical gamepads are ordered by their physical USB path so the */
/*  same Deck ports consistently become NES Player 1 and Player 2.   */
/*                                                                   */
/*===================================================================*/

#include <algorithm>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <linux/input.h>
#include <linux/kd.h>
#include <linux/keyboard.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <string>
#include <termios.h>
#include <time.h>
#include <sys/ioctl.h>
#include <unistd.h>
#include <vector>

/*-------------------------------------------------------------------*/
/*  NES Controller button definitions                                */
/*-------------------------------------------------------------------*/

#define PAD_A      (1u << 0)
#define PAD_B      (1u << 1)
#define PAD_SELECT (1u << 2)
#define PAD_START  (1u << 3)
#define PAD_UP     (1u << 4)
#define PAD_DOWN   (1u << 5)
#define PAD_LEFT   (1u << 6)
#define PAD_RIGHT  (1u << 7)
#define PAD_L      (1u << 8)
#define PAD_R      (1u << 9)

static const unsigned short kTheGamepadVendor = 0x1c59;
static const unsigned short kTheGamepadProduct = 0x0026;
static const size_t kPlayerCount = 2;

/*-------------------------------------------------------------------*/
/*  Input state                                                      */
/*-------------------------------------------------------------------*/

struct GamepadDevice {
  int fd;
  std::string path;
  std::string physical_path;
  struct input_absinfo x_info;
  struct input_absinfo y_info;
  uint32_t raw_buttons;
  int x_value;
  int y_value;
  unsigned int state;
  bool dropping_events;

  GamepadDevice()
      : fd(-1), raw_buttons(0), x_value(0), y_value(0), state(0),
        dropping_events(false) {
    memset(&x_info, 0, sizeof(x_info));
    memset(&y_info, 0, sizeof(y_info));
  }
};

struct GamepadCandidate {
  int fd;
  std::string path;
  std::string physical_path;
  struct input_absinfo x_info;
  struct input_absinfo y_info;

  GamepadCandidate() : fd(-1) {
    memset(&x_info, 0, sizeof(x_info));
    memset(&y_info, 0, sizeof(y_info));
  }
};

static int kb_fd = -1;
static bool kb_fd_owned = false;
static int kb_old_flags = -1;
static int old_kbd_mode = -1;
static struct termios old_term;
static bool keyboard_configured = false;
static unsigned int keyboard_state = 0;
static GamepadDevice gamepads[kPlayerCount];
static pthread_t input_thread;
static volatile int input_running = 0;
static int input_thread_started = 0;
static unsigned int pad_state[kPlayerCount] = {0, 0};
static unsigned int last_diagnostic_state[kPlayerCount] = {0, 0};
static bool input_diagnostics_enabled = false;
static pthread_mutex_t pad_mutex = PTHREAD_MUTEX_INITIALIZER;

/*-------------------------------------------------------------------*/
/*  Pure input mappings                                               */
/*-------------------------------------------------------------------*/

static unsigned int keycode_to_pad(unsigned char keycode) {
  switch (keycode) {
  case 0x67: return PAD_UP;    // Up arrow
  case 0x6c: return PAD_DOWN;  // Down arrow
  case 0x69: return PAD_LEFT;  // Left arrow
  case 0x6a: return PAD_RIGHT; // Right arrow
  case 0x11: return PAD_UP;    // W
  case 0x1f: return PAD_DOWN;  // S
  case 0x1e: return PAD_LEFT;  // A
  case 0x20: return PAD_RIGHT; // D
  case 0x2c: return PAD_A;     // Z
  case 0x24: return PAD_A;     // J
  case 0x2d: return PAD_B;     // X
  case 0x25: return PAD_B;     // K
  case 0x1c: return PAD_START; // Enter
  case 0x39: return PAD_SELECT; // Space
  case 0x2a: return PAD_SELECT; // Left Shift
  case 0x36: return PAD_SELECT; // Right Shift
  default: return 0;
  }
}

/*
 * Retro Games' published SDL mapping for USB 1c59:0026 is:
 *   Y=b0, B=b1, A=b2, X=b3, L=b4, R=b5, Back=b6, Start=b7.
 * The generic HID driver exposes b0..b7 as BTN_TRIGGER..BTN_BASE2.
 * Both pairs of face buttons remain useful on the two-button NES pad.
 */
static unsigned int gamepad_key_to_pad(unsigned short code) {
  switch (code) {
  case BTN_THUMB2: return PAD_A;  // Physical A
  case BTN_TOP: return PAD_A;     // Physical X
  case BTN_THUMB: return PAD_B;   // Physical B
  case BTN_TRIGGER: return PAD_B; // Physical Y
  case BTN_BASE: return PAD_SELECT; // Physical Back
  case BTN_BASE2: return PAD_START; // Physical Start
  case BTN_TOP2: return PAD_L;      // Physical L
  case BTN_PINKIE: return PAD_R;    // Physical R
  default: return 0;
  }
}

static unsigned int axis_to_pad(int value, int minimum, int maximum,
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

static unsigned int gamepad_state(const GamepadDevice &gamepad) {
  unsigned int state = 0;
  for (unsigned int index = 0; index < 8; ++index) {
    if (gamepad.raw_buttons & (1u << index))
      state |= gamepad_key_to_pad(
          static_cast<unsigned short>(BTN_TRIGGER + index));
  }
  state |= axis_to_pad(gamepad.x_value, gamepad.x_info.minimum,
                       gamepad.x_info.maximum, PAD_LEFT, PAD_RIGHT);
  state |= axis_to_pad(gamepad.y_value, gamepad.y_info.minimum,
                       gamepad.y_info.maximum, PAD_UP, PAD_DOWN);
  return state;
}

static bool bit_is_set(const unsigned long *bits, unsigned int bit) {
  const unsigned int bits_per_word = sizeof(unsigned long) * 8;
  return (bits[bit / bits_per_word] &
          (1UL << (bit % bits_per_word))) != 0;
}

static void publish_states(void) {
  const unsigned int next_state[kPlayerCount] = {
      keyboard_state | gamepads[0].state,
      gamepads[1].state,
  };
  bool changed[kPlayerCount] = {false, false};

  pthread_mutex_lock(&pad_mutex);
  for (size_t player = 0; player < kPlayerCount; ++player) {
    pad_state[player] = next_state[player];
    changed[player] = next_state[player] != last_diagnostic_state[player];
    last_diagnostic_state[player] = next_state[player];
  }
  pthread_mutex_unlock(&pad_mutex);

  if (input_diagnostics_enabled) {
    for (size_t player = 0; player < kPlayerCount; ++player) {
      if (changed[player])
        printf("InfoNES: input diagnostic P%zu state=0x%02x\n", player + 1,
               next_state[player]);
    }
    fflush(stdout);
  }
}

/*-------------------------------------------------------------------*/
/*  THEGamepad discovery and event handling                           */
/*-------------------------------------------------------------------*/

static bool is_event_name(const char *name) {
  if (!name || strncmp(name, "event", 5) != 0 || name[5] == '\0')
    return false;
  for (const char *cursor = name + 5; *cursor; ++cursor) {
    if (*cursor < '0' || *cursor > '9')
      return false;
  }
  return true;
}

static bool path_is_open(const std::string &path) {
  for (size_t player = 0; player < kPlayerCount; ++player) {
    if (gamepads[player].fd >= 0 && gamepads[player].path == path)
      return true;
  }
  return false;
}

static bool candidate_order(const GamepadCandidate &left,
                            const GamepadCandidate &right) {
  if (left.physical_path != right.physical_path)
    return left.physical_path < right.physical_path;
  return left.path < right.path;
}

static void close_gamepad(size_t player, bool remember_physical_path) {
  if (player >= kPlayerCount)
    return;
  GamepadDevice &gamepad = gamepads[player];
  if (gamepad.fd >= 0)
    close(gamepad.fd);
  gamepad.fd = -1;
  gamepad.path.clear();
  if (!remember_physical_path)
    gamepad.physical_path.clear();
  gamepad.raw_buttons = 0;
  gamepad.state = 0;
  gamepad.dropping_events = false;
  publish_states();
}

static bool resynchronize_gamepad(GamepadDevice *gamepad) {
  if (!gamepad || gamepad->fd < 0)
    return false;

  const size_t bits_per_word = sizeof(unsigned long) * 8;
  const size_t key_words = (KEY_MAX + bits_per_word) / bits_per_word;
  std::vector<unsigned long> keys(key_words, 0);
  struct input_absinfo x_info;
  struct input_absinfo y_info;
  memset(&x_info, 0, sizeof(x_info));
  memset(&y_info, 0, sizeof(y_info));
  if (ioctl(gamepad->fd, EVIOCGKEY(keys.size() * sizeof(keys[0])), &keys[0]) < 0 ||
      ioctl(gamepad->fd, EVIOCGABS(ABS_X), &x_info) < 0 ||
      ioctl(gamepad->fd, EVIOCGABS(ABS_Y), &y_info) < 0)
    return false;

  gamepad->x_info = x_info;
  gamepad->y_info = y_info;
  gamepad->x_value = x_info.value;
  gamepad->y_value = y_info.value;
  gamepad->raw_buttons = 0;
  for (unsigned int index = 0; index < 8; ++index) {
    if (bit_is_set(&keys[0], BTN_TRIGGER + index))
      gamepad->raw_buttons |= 1u << index;
  }
  gamepad->state = gamepad_state(*gamepad);
  publish_states();
  return true;
}

static void attach_candidate(size_t player, GamepadCandidate *candidate) {
  if (!candidate || candidate->fd < 0 || player >= kPlayerCount)
    return;
  GamepadDevice &gamepad = gamepads[player];
  gamepad.fd = candidate->fd;
  candidate->fd = -1;
  gamepad.path = candidate->path;
  gamepad.physical_path = candidate->physical_path;
  gamepad.x_info = candidate->x_info;
  gamepad.y_info = candidate->y_info;
  gamepad.x_value = candidate->x_info.value;
  gamepad.y_value = candidate->y_info.value;
  gamepad.raw_buttons = 0;
  gamepad.state = 0;
  gamepad.dropping_events = false;
  resynchronize_gamepad(&gamepad);
  printf("InfoNES: Player %zu THEGamepad on %s (%s)\n", player + 1,
         gamepad.path.c_str(), gamepad.physical_path.c_str());
}

static void scan_gamepads(void) {
  DIR *directory = opendir("/dev/input");
  if (!directory)
    return;

  std::vector<GamepadCandidate> candidates;
  for (struct dirent *entry = readdir(directory); entry;
       entry = readdir(directory)) {
    if (!is_event_name(entry->d_name))
      continue;
    const std::string path = std::string("/dev/input/") + entry->d_name;
    if (path_is_open(path))
      continue;
    const int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0)
      continue;

    struct input_id id;
    struct input_absinfo x_info;
    struct input_absinfo y_info;
    memset(&id, 0, sizeof(id));
    memset(&x_info, 0, sizeof(x_info));
    memset(&y_info, 0, sizeof(y_info));
    if (ioctl(fd, EVIOCGID, &id) < 0 || id.vendor != kTheGamepadVendor ||
        id.product != kTheGamepadProduct ||
        ioctl(fd, EVIOCGABS(ABS_X), &x_info) < 0 ||
        ioctl(fd, EVIOCGABS(ABS_Y), &y_info) < 0) {
      close(fd);
      continue;
    }

    char physical_path[PATH_MAX];
    memset(physical_path, 0, sizeof(physical_path));
    if (ioctl(fd, EVIOCGPHYS(sizeof(physical_path)), physical_path) < 0 ||
        physical_path[0] == '\0')
      snprintf(physical_path, sizeof(physical_path), "%s", path.c_str());

    GamepadCandidate candidate;
    candidate.fd = fd;
    candidate.path = path;
    candidate.physical_path = physical_path;
    candidate.x_info = x_info;
    candidate.y_info = y_info;
    candidates.push_back(candidate);
  }
  closedir(directory);

  std::sort(candidates.begin(), candidates.end(), candidate_order);

  // Reconnect a controller to its remembered physical-player slot first.
  for (size_t player = 0; player < kPlayerCount; ++player) {
    if (gamepads[player].fd >= 0 || gamepads[player].physical_path.empty())
      continue;
    for (size_t index = 0; index < candidates.size(); ++index) {
      if (candidates[index].fd >= 0 && candidates[index].physical_path ==
                                             gamepads[player].physical_path) {
        attach_candidate(player, &candidates[index]);
        break;
      }
    }
  }

  // Fill never-used slots in physical USB-path order.
  for (size_t player = 0; player < kPlayerCount; ++player) {
    if (gamepads[player].fd >= 0 || !gamepads[player].physical_path.empty())
      continue;
    for (size_t index = 0; index < candidates.size(); ++index) {
      if (candidates[index].fd >= 0) {
        attach_candidate(player, &candidates[index]);
        break;
      }
    }
  }

  // If a controller moved ports during a game, reuse the first disconnected
  // slot without disturbing any controller that is still connected.
  for (size_t index = 0; index < candidates.size(); ++index) {
    if (candidates[index].fd < 0)
      continue;
    for (size_t player = 0; player < kPlayerCount; ++player) {
      if (gamepads[player].fd < 0) {
        gamepads[player].physical_path.clear();
        attach_candidate(player, &candidates[index]);
        break;
      }
    }
  }

  for (size_t index = 0; index < candidates.size(); ++index) {
    if (candidates[index].fd >= 0)
      close(candidates[index].fd);
  }
}

static bool drain_gamepad(size_t player) {
  if (player >= kPlayerCount || gamepads[player].fd < 0)
    return false;
  GamepadDevice &gamepad = gamepads[player];

  while (true) {
    struct input_event events[32];
    const ssize_t amount = read(gamepad.fd, events, sizeof(events));
    if (amount < 0) {
      if (errno == EINTR)
        continue;
      if (errno == EAGAIN || errno == EWOULDBLOCK)
        return true;
      return false;
    }
    if (amount == 0 ||
        amount % static_cast<ssize_t>(sizeof(struct input_event)) != 0)
      return false;

    const size_t count = static_cast<size_t>(amount) / sizeof(events[0]);
    for (size_t index = 0; index < count; ++index) {
      const struct input_event &event = events[index];
      if (gamepad.dropping_events) {
        if (event.type == EV_SYN && event.code == SYN_REPORT) {
          if (!resynchronize_gamepad(&gamepad))
            return false;
          gamepad.dropping_events = false;
        }
        continue;
      }
      if (event.type == EV_SYN && event.code == SYN_DROPPED) {
        gamepad.dropping_events = true;
      } else if (event.type == EV_KEY && event.code >= BTN_TRIGGER &&
                 event.code <= BTN_BASE2) {
        const uint32_t bit = 1u << (event.code - BTN_TRIGGER);
        if (event.value)
          gamepad.raw_buttons |= bit;
        else
          gamepad.raw_buttons &= ~bit;
      } else if (event.type == EV_ABS && event.code == ABS_X) {
        gamepad.x_value = event.value;
      } else if (event.type == EV_ABS && event.code == ABS_Y) {
        gamepad.y_value = event.value;
      }
    }
    gamepad.state = gamepad_state(gamepad);
    publish_states();
  }
}

/*-------------------------------------------------------------------*/
/*  Raw keyboard setup and handling                                   */
/*-------------------------------------------------------------------*/

static bool is_keyboard(int fd) {
  int data = 0;
  if (ioctl(fd, KDGKBTYPE, &data) != 0)
    return false;
  return data == KB_84 || data == KB_101;
}

static bool initialize_keyboard(void) {
  const char *tty_files[] = {"/dev/tty", "/dev/tty0", "/dev/console", NULL};
  for (int index = 0; tty_files[index] != NULL; ++index) {
    const int candidate = open(tty_files[index], O_RDONLY | O_CLOEXEC);
    if (candidate < 0)
      continue;
    if (is_keyboard(candidate)) {
      kb_fd = candidate;
      kb_fd_owned = true;
      printf("InfoNES: Using keyboard on %s\n", tty_files[index]);
      break;
    }
    close(candidate);
  }

  if (kb_fd < 0) {
    for (int fd = 0; fd < 3; ++fd) {
      if (is_keyboard(fd)) {
        kb_fd = fd;
        kb_fd_owned = false;
        printf("InfoNES: Using keyboard on fd %d\n", fd);
        break;
      }
    }
  }
  if (kb_fd < 0)
    return false;

  if (ioctl(kb_fd, KDGKBMODE, &old_kbd_mode) != 0 ||
      tcgetattr(kb_fd, &old_term) != 0) {
    if (kb_fd_owned)
      close(kb_fd);
    kb_fd = -1;
    kb_fd_owned = false;
    old_kbd_mode = -1;
    return false;
  }

  struct termios new_term = old_term;
  new_term.c_iflag = 0;
  new_term.c_lflag &= ~(ECHO | ICANON | ISIG);
  if (tcsetattr(kb_fd, TCSAFLUSH, &new_term) != 0 ||
      ioctl(kb_fd, KDSKBMODE, K_MEDIUMRAW) != 0) {
    tcsetattr(kb_fd, TCSAFLUSH, &old_term);
    if (kb_fd_owned)
      close(kb_fd);
    kb_fd = -1;
    kb_fd_owned = false;
    old_kbd_mode = -1;
    return false;
  }

  kb_old_flags = fcntl(kb_fd, F_GETFL, 0);
  if (kb_old_flags < 0 || fcntl(kb_fd, F_SETFL, kb_old_flags | O_NONBLOCK) != 0) {
    ioctl(kb_fd, KDSKBMODE, old_kbd_mode);
    tcsetattr(kb_fd, TCSAFLUSH, &old_term);
    if (kb_fd_owned)
      close(kb_fd);
    kb_fd = -1;
    kb_fd_owned = false;
    old_kbd_mode = -1;
    kb_old_flags = -1;
    return false;
  }
  keyboard_configured = true;
  return true;
}

static bool drain_keyboard(void) {
  while (kb_fd >= 0) {
    unsigned char data[64];
    const ssize_t amount = read(kb_fd, data, sizeof(data));
    if (amount < 0) {
      if (errno == EINTR)
        continue;
      if (errno == EAGAIN || errno == EWOULDBLOCK)
        return true;
      return false;
    }
    if (amount == 0)
      return false;
    for (ssize_t index = 0; index < amount; ++index) {
      const bool released = (data[index] & 0x80) != 0;
      const unsigned int button = keycode_to_pad(data[index] & 0x7f);
      if (!button)
        continue;
      if (released)
        keyboard_state &= ~button;
      else
        keyboard_state |= button;
    }
    publish_states();
  }
  return false;
}

static void close_keyboard(void) {
  if (keyboard_configured && kb_fd >= 0) {
    if (kb_old_flags >= 0)
      fcntl(kb_fd, F_SETFL, kb_old_flags);
    if (old_kbd_mode != -1)
      ioctl(kb_fd, KDSKBMODE, old_kbd_mode);
    tcsetattr(kb_fd, TCSAFLUSH, &old_term);
  }
  if (kb_fd_owned && kb_fd >= 0)
    close(kb_fd);
  kb_fd = -1;
  kb_fd_owned = false;
  kb_old_flags = -1;
  old_kbd_mode = -1;
  keyboard_configured = false;
  keyboard_state = 0;
  publish_states();
}

static int64_t monotonic_milliseconds(void) {
  struct timespec now;
  if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
    return 0;
  return static_cast<int64_t>(now.tv_sec) * 1000 + now.tv_nsec / 1000000;
}

/*-------------------------------------------------------------------*/
/*  Input thread                                                     */
/*-------------------------------------------------------------------*/

static void *input_thread_func(void *) {
  int64_t last_scan = 0;
  while (input_running) {
    const int64_t now = monotonic_milliseconds();
    if (now - last_scan >= 1000) {
      scan_gamepads();
      last_scan = now;
    }

    struct pollfd descriptors[1 + kPlayerCount];
    int descriptor_player[1 + kPlayerCount];
    nfds_t count = 0;
    if (kb_fd >= 0) {
      descriptors[count].fd = kb_fd;
      descriptors[count].events = POLLIN;
      descriptors[count].revents = 0;
      descriptor_player[count] = -1;
      ++count;
    }
    for (size_t player = 0; player < kPlayerCount; ++player) {
      if (gamepads[player].fd < 0)
        continue;
      descriptors[count].fd = gamepads[player].fd;
      descriptors[count].events = POLLIN;
      descriptors[count].revents = 0;
      descriptor_player[count] = static_cast<int>(player);
      ++count;
    }

    const int result = poll(count ? descriptors : NULL, count, 100);
    if (result < 0) {
      if (errno == EINTR)
        continue;
      usleep(100000);
      continue;
    }
    if (result == 0)
      continue;

    for (nfds_t index = 0; index < count; ++index) {
      if (!(descriptors[index].revents & (POLLIN | POLLERR | POLLHUP | POLLNVAL)))
        continue;
      const int player = descriptor_player[index];
      if (player < 0) {
        if (!drain_keyboard()) {
          printf("InfoNES: Keyboard input disconnected\n");
          close_keyboard();
        }
      } else if (!drain_gamepad(static_cast<size_t>(player))) {
        printf("InfoNES: Player %d gamepad disconnected\n", player + 1);
        close_gamepad(static_cast<size_t>(player), true);
      }
    }
  }
  return NULL;
}

/*-------------------------------------------------------------------*/
/*  Cleanup                                                          */
/*-------------------------------------------------------------------*/

static void input_cleanup(void) {
  input_running = 0;
  if (input_thread_started) {
    pthread_join(input_thread, NULL);
    input_thread_started = 0;
  }

  close_keyboard();
  for (size_t player = 0; player < kPlayerCount; ++player)
    close_gamepad(player, false);
}

/*===================================================================*/
/*                     InitJoypadInput()                              */
/*===================================================================*/

extern "C" int InitJoypadInput(void) {
  printf("InfoNES: Initializing keyboard and two-player gamepad input...\n");
  atexit(input_cleanup);

  const char *diagnostics = getenv("INFONES_INPUT_DIAGNOSTICS");
  input_diagnostics_enabled = diagnostics && strcmp(diagnostics, "1") == 0;
  if (input_diagnostics_enabled)
    printf("InfoNES: Input state diagnostics enabled\n");

  if (!initialize_keyboard())
    printf("InfoNES: No raw keyboard available; gamepads remain enabled\n");
  scan_gamepads();

  size_t gamepad_count = 0;
  for (size_t player = 0; player < kPlayerCount; ++player)
    gamepad_count += gamepads[player].fd >= 0 ? 1 : 0;
  printf("InfoNES: %zu THEGamepad controller(s) ready\n", gamepad_count);
  printf("InfoNES: THEGamepad D-pad=move, A/X=primary, B/Y=secondary, "
         "L/R=shoulders, Back=Select, Start=Start\n");
  printf("InfoNES: Keyboard Arrows/WASD=move, Z/J=A, X/K=B, "
         "Space=Select, Enter=Start\n");

  input_running = 1;
  if (pthread_create(&input_thread, NULL, input_thread_func, NULL) != 0) {
    input_running = 0;
    printf("InfoNES: Cannot start input thread\n");
    input_cleanup();
    return -1;
  }
  input_thread_started = 1;
  return 0;
}

/*===================================================================*/
/*                     GetJoypadInput()                               */
/*===================================================================*/

extern "C" unsigned int GetJoypadInput(unsigned int player) {
  if (player >= kPlayerCount)
    return 0;
  pthread_mutex_lock(&pad_mutex);
  const unsigned int state = pad_state[player];
  pthread_mutex_unlock(&pad_mutex);
  return state;
}

/* End of joypad_input.cpp */
