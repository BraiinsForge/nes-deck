/*===================================================================*/
/*                                                                   */
/*  joypad_input.cpp : Input handler for Braiins Forge Deck          */
/*                                                                   */
/*  Uses TTY raw keyboard input (same as fbDOOM)                     */
/*                                                                   */
/*===================================================================*/

#include <fcntl.h>
#include <linux/kd.h>
#include <linux/keyboard.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <sys/ioctl.h>
#include <unistd.h>

/*-------------------------------------------------------------------*/
/*  NES Controller button definitions                                */
/*-------------------------------------------------------------------*/

#define PAD_A      (1 << 0)
#define PAD_B      (1 << 1)
#define PAD_SELECT (1 << 2)
#define PAD_START  (1 << 3)
#define PAD_UP     (1 << 4)
#define PAD_DOWN   (1 << 5)
#define PAD_LEFT   (1 << 6)
#define PAD_RIGHT  (1 << 7)

/*-------------------------------------------------------------------*/
/*  Input state                                                      */
/*-------------------------------------------------------------------*/

static int kb_fd = -1;
static int old_kbd_mode = -1;
static struct termios old_term;
static pthread_t input_thread;
static volatile int input_running = 0;
static volatile unsigned int pad_state = 0;
static pthread_mutex_t pad_mutex = PTHREAD_MUTEX_INITIALIZER;

/*-------------------------------------------------------------------*/
/*  Check if fd is a keyboard                                        */
/*-------------------------------------------------------------------*/

static int is_keyboard(int fd) {
  int data = 0;
  if (ioctl(fd, KDGKBTYPE, &data) != 0)
    return 0;
  return (data == KB_84 || data == KB_101);
}

/*-------------------------------------------------------------------*/
/*  Map raw keycode to NES button                                    */
/*-------------------------------------------------------------------*/

static unsigned int keycode_to_pad(unsigned char keycode) {
  switch (keycode) {
  // Arrow keys (raw scancodes)
  case 0x67: return PAD_UP;    // Up
  case 0x6c: return PAD_DOWN;  // Down
  case 0x69: return PAD_LEFT;  // Left
  case 0x6a: return PAD_RIGHT; // Right

  // WASD
  case 0x11: return PAD_UP;    // W
  case 0x1f: return PAD_DOWN;  // S
  case 0x1e: return PAD_LEFT;  // A
  case 0x20: return PAD_RIGHT; // D

  // Z/X for A/B
  case 0x2c: return PAD_A; // Z
  case 0x2d: return PAD_B; // X

  // J/K for A/B
  case 0x24: return PAD_A; // J
  case 0x25: return PAD_B; // K

  // Start/Select
  case 0x1c: return PAD_START;  // Enter
  case 0x39: return PAD_SELECT; // Space
  case 0x2a: return PAD_SELECT; // Left Shift
  case 0x36: return PAD_SELECT; // Right Shift

  default:
    return 0;
  }
}

/*-------------------------------------------------------------------*/
/*  Input thread                                                     */
/*-------------------------------------------------------------------*/

static void *input_thread_func(void *arg) {
  unsigned char data;

  while (input_running) {
    if (read(kb_fd, &data, 1) == 1) {
      int released = (data & 0x80) != 0;
      unsigned char keycode = data & 0x7F;

      unsigned int button = keycode_to_pad(keycode);
      if (button) {
        pthread_mutex_lock(&pad_mutex);
        if (released) {
          pad_state &= ~button;
        } else {
          pad_state |= button;
        }
        pthread_mutex_unlock(&pad_mutex);
      }
    } else {
      usleep(1000);
    }
  }

  return NULL;
}

/*-------------------------------------------------------------------*/
/*  Cleanup on exit                                                  */
/*-------------------------------------------------------------------*/

static void kbd_cleanup(void) {
  if (old_kbd_mode != -1 && kb_fd >= 0) {
    ioctl(kb_fd, KDSKBMODE, old_kbd_mode);
    tcsetattr(kb_fd, TCSAFLUSH, &old_term);
  }
}

/*===================================================================*/
/*                     InitJoypadInput()                             */
/*===================================================================*/

extern "C" int InitJoypadInput(void) {
  const char *tty_files[] = {"/dev/tty", "/dev/tty0", "/dev/console", NULL};
  struct termios new_term;
  int flags;

  printf("InfoNES: Initializing TTY keyboard input...\n");

  // Find a keyboard TTY
  for (int i = 0; tty_files[i] != NULL; i++) {
    kb_fd = open(tty_files[i], O_RDONLY);
    if (kb_fd < 0)
      continue;

    if (is_keyboard(kb_fd)) {
      printf("InfoNES: Using keyboard on %s\n", tty_files[i]);
      break;
    }
    close(kb_fd);
    kb_fd = -1;
  }

  // Try stdin/stdout/stderr
  if (kb_fd < 0) {
    for (int fd = 0; fd < 3; fd++) {
      if (is_keyboard(fd)) {
        kb_fd = fd;
        printf("InfoNES: Using keyboard on fd %d\n", fd);
        break;
      }
    }
  }

  if (kb_fd < 0) {
    printf("InfoNES: No keyboard found!\n");
    return -1;
  }

  // Save old keyboard mode
  if (ioctl(kb_fd, KDGKBMODE, &old_kbd_mode) != 0) {
    printf("InfoNES: Cannot get keyboard mode\n");
    return -1;
  }

  // Save old terminal settings
  if (tcgetattr(kb_fd, &old_term) != 0) {
    printf("InfoNES: Cannot get terminal settings\n");
    return -1;
  }

  // Set up cleanup
  atexit(kbd_cleanup);

  // Configure terminal for raw input
  new_term = old_term;
  new_term.c_iflag = 0;
  new_term.c_lflag &= ~(ECHO | ICANON | ISIG);
  tcsetattr(kb_fd, TCSAFLUSH, &new_term);

  // Set keyboard to mediumraw mode
  if (ioctl(kb_fd, KDSKBMODE, K_MEDIUMRAW) != 0) {
    printf("InfoNES: Cannot set mediumraw mode\n");
    tcsetattr(kb_fd, TCSAFLUSH, &old_term);
    return -1;
  }

  // Non-blocking mode
  flags = fcntl(kb_fd, F_GETFL, 0);
  fcntl(kb_fd, F_SETFL, flags | O_NONBLOCK);

  printf("InfoNES: Keyboard ready\n");
  printf("InfoNES: Controls: Arrows/WASD=D-pad, Z/X or J/K=A/B, Enter=Start, "
         "Space=Select\n");

  // Start input thread
  input_running = 1;
  pthread_create(&input_thread, NULL, input_thread_func, NULL);

  return 0;
}

/*===================================================================*/
/*                     GetJoypadInput()                              */
/*===================================================================*/

extern "C" int GetJoypadInput(void) {
  unsigned int state;

  pthread_mutex_lock(&pad_mutex);
  state = pad_state;
  pthread_mutex_unlock(&pad_mutex);

  return state;
}

/*
 * End of joypad_input.cpp
 */
