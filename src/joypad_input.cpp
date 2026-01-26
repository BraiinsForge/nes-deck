/*===================================================================*/
/*                                                                   */
/*  joypad_input.cpp : Input handler for Braiins Forge Deck          */
/*                                                                   */
/*  Supports keyboard via /dev/input/event* and USB joystick         */
/*                                                                   */
/*===================================================================*/

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <dirent.h>
#include <pthread.h>
#include <sys/select.h>
#include <linux/input.h>
#include <linux/joystick.h>

/*-------------------------------------------------------------------*/
/*  NES Controller button definitions                                */
/*-------------------------------------------------------------------*/

#define PAD_A       (1 << 0)
#define PAD_B       (1 << 1)
#define PAD_SELECT  (1 << 2)
#define PAD_START   (1 << 3)
#define PAD_UP      (1 << 4)
#define PAD_DOWN    (1 << 5)
#define PAD_LEFT    (1 << 6)
#define PAD_RIGHT   (1 << 7)

/*-------------------------------------------------------------------*/
/*  Input device file descriptors                                    */
/*-------------------------------------------------------------------*/

static int keyboard_fd = -1;
static int joystick_fd = -1;
static pthread_t input_thread;
static volatile int input_running = 0;
static volatile unsigned int pad_state = 0;
static pthread_mutex_t pad_mutex = PTHREAD_MUTEX_INITIALIZER;

/*-------------------------------------------------------------------*/
/*  Find and open input device                                       */
/*-------------------------------------------------------------------*/

static int open_keyboard(void)
{
    DIR *dir;
    struct dirent *entry;
    char path[256];
    char name[256];
    int fd;

    dir = opendir("/dev/input");
    if (!dir) {
        return -1;
    }

    while ((entry = readdir(dir)) != NULL) {
        if (strncmp(entry->d_name, "event", 5) != 0)
            continue;

        snprintf(path, sizeof(path), "/dev/input/%s", entry->d_name);

        fd = open(path, O_RDONLY | O_NONBLOCK);
        if (fd < 0)
            continue;

        // Check if this device has keyboard capabilities
        unsigned long evbit = 0;
        if (ioctl(fd, EVIOCGBIT(0, sizeof(evbit)), &evbit) >= 0) {
            if (evbit & (1 << EV_KEY)) {
                // Check for keyboard keys
                unsigned long keybit[KEY_MAX / 8 / sizeof(long) + 1] = {0};
                if (ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(keybit)), keybit) >= 0) {
                    // Check if common keyboard keys exist
                    if ((keybit[KEY_A / (8 * sizeof(long))] & (1UL << (KEY_A % (8 * sizeof(long))))) ||
                        (keybit[KEY_UP / (8 * sizeof(long))] & (1UL << (KEY_UP % (8 * sizeof(long)))))) {
                        if (ioctl(fd, EVIOCGNAME(sizeof(name)), name) >= 0) {
                            printf("InfoNES: Using keyboard: %s (%s)\n", name, path);
                        }
                        closedir(dir);
                        return fd;
                    }
                }
            }
        }
        close(fd);
    }

    closedir(dir);
    return -1;
}

static int open_joystick(void)
{
    char path[64];
    int fd;

    // Try common joystick paths
    const char *js_paths[] = {
        "/dev/input/js0",
        "/dev/input/js1",
        NULL
    };

    for (int i = 0; js_paths[i] != NULL; i++) {
        fd = open(js_paths[i], O_RDONLY | O_NONBLOCK);
        if (fd >= 0) {
            char name[128] = "Unknown";
            ioctl(fd, JSIOCGNAME(sizeof(name)), name);
            printf("InfoNES: Using joystick: %s (%s)\n", name, js_paths[i]);
            return fd;
        }
    }

    return -1;
}

/*-------------------------------------------------------------------*/
/*  Map keyboard key to NES button                                   */
/*-------------------------------------------------------------------*/

static unsigned int key_to_pad(int key, int pressed)
{
    unsigned int button = 0;

    switch (key) {
        // Arrow keys for D-pad
        case KEY_UP:        button = PAD_UP; break;
        case KEY_DOWN:      button = PAD_DOWN; break;
        case KEY_LEFT:      button = PAD_LEFT; break;
        case KEY_RIGHT:     button = PAD_RIGHT; break;

        // WASD alternative
        case KEY_W:         button = PAD_UP; break;
        case KEY_S:         button = PAD_DOWN; break;
        case KEY_A:         button = PAD_LEFT; break;
        case KEY_D:         button = PAD_RIGHT; break;

        // Z/X for A/B buttons
        case KEY_Z:         button = PAD_A; break;
        case KEY_X:         button = PAD_B; break;

        // J/K alternative for A/B
        case KEY_J:         button = PAD_A; break;
        case KEY_K:         button = PAD_B; break;

        // Enter for Start, Shift/Space for Select
        case KEY_ENTER:     button = PAD_START; break;
        case KEY_SPACE:     button = PAD_SELECT; break;
        case KEY_RIGHTSHIFT:
        case KEY_LEFTSHIFT: button = PAD_SELECT; break;

        default:
            return 0;
    }

    return button;
}

/*-------------------------------------------------------------------*/
/*  Input polling thread                                             */
/*-------------------------------------------------------------------*/

static void *input_thread_func(void *arg)
{
    struct input_event ev;
    fd_set fds;
    struct timeval tv;
    int maxfd;

    while (input_running) {
        FD_ZERO(&fds);
        maxfd = 0;

        if (keyboard_fd >= 0) {
            FD_SET(keyboard_fd, &fds);
            if (keyboard_fd > maxfd) maxfd = keyboard_fd;
        }

        if (joystick_fd >= 0) {
            FD_SET(joystick_fd, &fds);
            if (joystick_fd > maxfd) maxfd = joystick_fd;
        }

        if (maxfd == 0) {
            usleep(10000);
            continue;
        }

        tv.tv_sec = 0;
        tv.tv_usec = 10000;

        if (select(maxfd + 1, &fds, NULL, NULL, &tv) <= 0) {
            continue;
        }

        // Handle keyboard events
        if (keyboard_fd >= 0 && FD_ISSET(keyboard_fd, &fds)) {
            while (read(keyboard_fd, &ev, sizeof(ev)) == sizeof(ev)) {
                if (ev.type == EV_KEY) {
                    unsigned int button = key_to_pad(ev.code, ev.value);
                    if (button) {
                        pthread_mutex_lock(&pad_mutex);
                        if (ev.value) {
                            pad_state |= button;
                        } else {
                            pad_state &= ~button;
                        }
                        pthread_mutex_unlock(&pad_mutex);
                    }
                }
            }
        }

        // Handle joystick events (js interface)
        if (joystick_fd >= 0 && FD_ISSET(joystick_fd, &fds)) {
            struct js_event js;

            while (read(joystick_fd, &js, sizeof(js)) == sizeof(js)) {
                pthread_mutex_lock(&pad_mutex);

                // Axis events
                if ((js.type & 0x02) && js.number < 2) {
                    if (js.number == 0) {  // X axis
                        pad_state &= ~(PAD_LEFT | PAD_RIGHT);
                        if (js.value < -16384) pad_state |= PAD_LEFT;
                        else if (js.value > 16384) pad_state |= PAD_RIGHT;
                    } else {  // Y axis
                        pad_state &= ~(PAD_UP | PAD_DOWN);
                        if (js.value < -16384) pad_state |= PAD_UP;
                        else if (js.value > 16384) pad_state |= PAD_DOWN;
                    }
                }

                // Button events
                if (js.type & 0x01) {
                    unsigned int button = 0;
                    switch (js.number) {
                        case 0: button = PAD_A; break;       // Usually A/Cross
                        case 1: button = PAD_B; break;       // Usually B/Circle
                        case 2: button = PAD_SELECT; break;  // Select/Share
                        case 3: button = PAD_START; break;   // Start/Options
                        case 4: button = PAD_A; break;       // L1 -> A
                        case 5: button = PAD_B; break;       // R1 -> B
                    }
                    if (button) {
                        if (js.value) {
                            pad_state |= button;
                        } else {
                            pad_state &= ~button;
                        }
                    }
                }

                pthread_mutex_unlock(&pad_mutex);
            }
        }
    }

    return NULL;
}

/*===================================================================*/
/*                     InitJoypadInput()                             */
/*===================================================================*/

extern "C" int InitJoypadInput(void)
{
    printf("InfoNES: Initializing input devices...\n");

    keyboard_fd = open_keyboard();
    joystick_fd = open_joystick();

    if (keyboard_fd < 0 && joystick_fd < 0) {
        printf("InfoNES: Warning - No input devices found!\n");
        printf("InfoNES: Controls: Arrow keys/WASD = D-pad, Z/X = A/B, Enter = Start, Space = Select\n");
    }

    input_running = 1;
    pthread_create(&input_thread, NULL, input_thread_func, NULL);

    return 0;
}

/*===================================================================*/
/*                     GetJoypadInput()                              */
/*===================================================================*/

extern "C" int GetJoypadInput(void)
{
    unsigned int state;

    pthread_mutex_lock(&pad_mutex);
    state = pad_state;
    pthread_mutex_unlock(&pad_mutex);

    return state;
}

/*
 * End of joypad_input.cpp
 */
