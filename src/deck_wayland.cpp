#include "deck_wayland.h"

#include "deck-widget-v1-client-protocol.h"
#include "wlr-layer-shell-unstable-v1-client-protocol.h"

#include <wayland-client.h>

#include <algorithm>
#include <cerrno>
#include <climits>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <poll.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

extern "C" {
// The layer-shell get_popup request refers to xdg_popup. Retro Deck never
// creates popups, but the generated protocol type table still needs the
// external interface descriptor at link time.
extern const wl_interface xdg_popup_interface = {"xdg_popup", 3, 0, NULL, 0,
                                                  NULL};
}

namespace {

const unsigned int kDisplayWidth = 1280;
const unsigned int kDisplayHeight = 480;
const unsigned int kSafeInset = 16;
const size_t kBufferCount = 3;
const int kConfigureTimeoutMs = 2000;

std::string errno_message(const std::string &prefix) {
  return prefix + ": " + std::strerror(errno);
}

int create_anonymous_file(size_t size, std::string *error) {
  if (size == 0 || size > static_cast<size_t>(LLONG_MAX)) {
    if (error)
      *error = "invalid Wayland shared memory size";
    return -1;
  }

  int fd = -1;
#if defined(SYS_memfd_create)
  fd = static_cast<int>(
      syscall(SYS_memfd_create, "retro-deck-wayland", O_CLOEXEC));
#endif
  if (fd < 0) {
    const char *runtime = std::getenv("XDG_RUNTIME_DIR");
    const std::string directory = runtime && runtime[0] ? runtime : "/tmp";
    std::string pattern = directory + "/retro-deck-wayland-XXXXXX";
    std::vector<char> path(pattern.begin(), pattern.end());
    path.push_back('\0');
    fd = mkstemp(&path[0]);
    if (fd >= 0) {
      unlink(&path[0]);
      const int flags = fcntl(fd, F_GETFD);
      if (flags >= 0)
        fcntl(fd, F_SETFD, flags | FD_CLOEXEC);
    }
  }
  if (fd < 0) {
    if (error)
      *error = errno_message("cannot create Wayland shared memory file");
    return -1;
  }
  if (ftruncate(fd, static_cast<off_t>(size)) != 0) {
    if (error)
      *error = errno_message("cannot size Wayland shared memory file");
    close(fd);
    return -1;
  }
  return fd;
}

uint32_t rgb565_to_xrgb(uint16_t pixel) {
  const uint32_t red = (pixel >> 11) & 0x1f;
  const uint32_t green = (pixel >> 5) & 0x3f;
  const uint32_t blue = pixel & 0x1f;
  return 0xff000000U | ((red * 255U / 31U) << 16) |
         ((green * 255U / 63U) << 8) | (blue * 255U / 31U);
}

void compute_game_size(unsigned int source_width, unsigned int source_height,
                       unsigned int *width, unsigned int *height) {
  const unsigned int available_width = kDisplayWidth - 2 * kSafeInset;
  const unsigned int available_height = kDisplayHeight - 2 * kSafeInset;
  const unsigned int horizontal = available_width / source_width;
  const unsigned int vertical = available_height / source_height;
  const unsigned int scale = std::max(1U, std::min(horizontal, vertical));
  *width = source_width * scale;
  *height = source_height * scale;
}

} // namespace

struct DeckWaylandPresentation::Impl {
  struct BufferSlot {
    BufferSlot()
        : owner(NULL), buffer(NULL), memory(NULL), size(0), busy(false) {}
    Impl *owner;
    wl_buffer *buffer;
    uint32_t *memory;
    size_t size;
    bool busy;
  };

  Impl()
      : display(NULL), registry(NULL), compositor(NULL), shm(NULL), seat(NULL),
        touch(NULL), widget_manager(NULL), widget_surface(NULL),
        layer_shell(NULL), surface(NULL), background_surface(NULL),
        game_layer(NULL), background_layer(NULL), black_buffer(NULL),
        black_memory(NULL), widget(false), configured(false),
        background_configured(false), lifecycle_visible(true), shutdown(false),
        width(0), height(0), touch_x(0), touch_y(0), touch_down(false) {}

  wl_display *display;
  wl_registry *registry;
  wl_compositor *compositor;
  wl_shm *shm;
  wl_seat *seat;
  wl_touch *touch;
  deck_widget_manager_v1 *widget_manager;
  deck_widget_surface_v1 *widget_surface;
  zwlr_layer_shell_v1 *layer_shell;
  wl_surface *surface;
  wl_surface *background_surface;
  zwlr_layer_surface_v1 *game_layer;
  zwlr_layer_surface_v1 *background_layer;
  wl_buffer *black_buffer;
  uint32_t *black_memory;
  bool widget;
  bool configured;
  bool background_configured;
  bool lifecycle_visible;
  bool shutdown;
  unsigned int width;
  unsigned int height;
  int touch_x;
  int touch_y;
  bool touch_down;
  std::vector<DeckWaylandTouchReport> touch_reports;
  std::vector<BufferSlot> slots;

  static void registry_global(void *data, wl_registry *registry, uint32_t name,
                              const char *interface, uint32_t version) {
    Impl *self = static_cast<Impl *>(data);
    if (std::strcmp(interface, wl_compositor_interface.name) == 0) {
      self->compositor = static_cast<wl_compositor *>(wl_registry_bind(
          registry, name, &wl_compositor_interface, std::min(version, 4U)));
    } else if (std::strcmp(interface, wl_shm_interface.name) == 0) {
      self->shm = static_cast<wl_shm *>(
          wl_registry_bind(registry, name, &wl_shm_interface, 1));
    } else if (std::strcmp(interface, wl_seat_interface.name) == 0) {
      self->seat = static_cast<wl_seat *>(wl_registry_bind(
          registry, name, &wl_seat_interface, std::min(version, 7U)));
      wl_seat_add_listener(self->seat, &seat_listener, self);
    } else if (std::strcmp(interface,
                           deck_widget_manager_v1_interface.name) == 0) {
      self->widget_manager = static_cast<deck_widget_manager_v1 *>(
          wl_registry_bind(registry, name, &deck_widget_manager_v1_interface,
                           1));
    } else if (std::strcmp(interface, zwlr_layer_shell_v1_interface.name) ==
               0) {
      self->layer_shell = static_cast<zwlr_layer_shell_v1 *>(wl_registry_bind(
          registry, name, &zwlr_layer_shell_v1_interface,
          std::min(version, 4U)));
    }
  }

  static void registry_remove(void *, wl_registry *, uint32_t) {}

  static void seat_capabilities(void *data, wl_seat *seat,
                                uint32_t capabilities) {
    Impl *self = static_cast<Impl *>(data);
    const bool have_touch =
        (capabilities & WL_SEAT_CAPABILITY_TOUCH) != 0;
    if (have_touch && !self->touch) {
      self->touch = wl_seat_get_touch(seat);
      wl_touch_add_listener(self->touch, &touch_listener, self);
    } else if (!have_touch && self->touch) {
      wl_touch_destroy(self->touch);
      self->touch = NULL;
      self->touch_down = false;
    }
  }

  static void seat_name(void *, wl_seat *, const char *) {}

  static void touch_down_event(void *data, wl_touch *, uint32_t, uint32_t,
                               wl_surface *target, int32_t,
                               wl_fixed_t x, wl_fixed_t y) {
    Impl *self = static_cast<Impl *>(data);
    if (target != self->surface)
      return;
    self->touch_x = wl_fixed_to_int(x);
    self->touch_y = wl_fixed_to_int(y);
    self->touch_down = true;
    self->push_touch(true, false);
  }

  static void touch_up_event(void *data, wl_touch *, uint32_t, uint32_t,
                             int32_t) {
    Impl *self = static_cast<Impl *>(data);
    if (!self->touch_down)
      return;
    self->touch_down = false;
    self->push_touch(false, true);
  }

  static void touch_motion_event(void *data, wl_touch *, uint32_t, int32_t,
                                 wl_fixed_t x, wl_fixed_t y) {
    Impl *self = static_cast<Impl *>(data);
    if (!self->touch_down)
      return;
    self->touch_x = wl_fixed_to_int(x);
    self->touch_y = wl_fixed_to_int(y);
    self->push_touch(false, false);
  }

  static void touch_frame_event(void *, wl_touch *) {}

  static void touch_cancel_event(void *data, wl_touch *) {
    Impl *self = static_cast<Impl *>(data);
    if (self->touch_down) {
      self->touch_down = false;
      DeckWaylandTouchReport report;
      report.x = -1;
      report.y = -1;
      report.down = false;
      report.pressed = false;
      report.released = true;
      self->touch_reports.push_back(report);
    }
  }

  static void touch_shape_event(void *, wl_touch *, int32_t, wl_fixed_t,
                                wl_fixed_t) {}
  static void touch_orientation_event(void *, wl_touch *, int32_t,
                                      wl_fixed_t) {}

  void push_touch(bool pressed, bool released) {
    DeckWaylandTouchReport report;
    report.x = std::max(0, std::min(static_cast<int>(width) - 1, touch_x));
    report.y = std::max(0, std::min(static_cast<int>(height) - 1, touch_y));
    report.down = touch_down;
    report.pressed = pressed;
    report.released = released;
    touch_reports.push_back(report);
  }

  static void widget_configure(void *data, deck_widget_surface_v1 *,
                               uint32_t width, uint32_t height, uint32_t,
                               const char *) {
    Impl *self = static_cast<Impl *>(data);
    self->width = width;
    self->height = height;
  }
  static void widget_display_info(void *, deck_widget_surface_v1 *, uint32_t,
                                  uint32_t, uint32_t, uint32_t) {}
  static void widget_params(void *, deck_widget_surface_v1 *, const char *) {}
  static void widget_configure_done(void *data, deck_widget_surface_v1 *) {
    static_cast<Impl *>(data)->configured = true;
  }
  static void widget_string(void *, deck_widget_surface_v1 *, const char *) {}
  static void widget_uint(void *, deck_widget_surface_v1 *, uint32_t) {}
  static void widget_alarm(void *, deck_widget_surface_v1 *, uint32_t, int32_t,
                           uint32_t, const char *) {}
  static void widget_lifecycle(void *data, deck_widget_surface_v1 *,
                               uint32_t state) {
    Impl *self = static_cast<Impl *>(data);
    self->lifecycle_visible = state != 0;
  }
  static void widget_shutdown(void *data, deck_widget_surface_v1 *) {
    static_cast<Impl *>(data)->shutdown = true;
  }
  static void widget_transition(void *, deck_widget_surface_v1 *) {}
  static void widget_led_status(void *, deck_widget_surface_v1 *, uint32_t,
                                uint32_t) {}

  static void layer_configure(void *data, zwlr_layer_surface_v1 *layer,
                              uint32_t serial, uint32_t width,
                              uint32_t height) {
    Impl *self = static_cast<Impl *>(data);
    zwlr_layer_surface_v1_ack_configure(layer, serial);
    if (layer == self->game_layer) {
      self->configured = true;
      if (width > 0)
        self->width = width;
      if (height > 0)
        self->height = height;
    } else if (layer == self->background_layer) {
      self->background_configured = true;
    }
  }

  static void layer_closed(void *data, zwlr_layer_surface_v1 *) {
    static_cast<Impl *>(data)->shutdown = true;
  }

  static void buffer_release(void *data, wl_buffer *) {
    static_cast<BufferSlot *>(data)->busy = false;
  }

  bool bind_globals(std::string *error) {
    display = wl_display_connect(NULL);
    if (!display) {
      if (error)
        *error = "cannot connect to the Wayland display";
      return false;
    }
    registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, this);
    if (wl_display_roundtrip(display) < 0) {
      if (error)
        *error = "cannot discover Wayland globals";
      return false;
    }
    if (!compositor || !shm) {
      if (error)
        *error = "Wayland compositor does not provide wl_compositor and wl_shm";
      return false;
    }
    return true;
  }

  bool wait_until_configured(bool wait_for_background, std::string *error) {
    int remaining = kConfigureTimeoutMs;
    while ((!configured || (wait_for_background && !background_configured)) &&
           !shutdown && remaining > 0) {
      struct pollfd descriptor;
      descriptor.fd = wl_display_get_fd(display);
      descriptor.events = POLLIN;
      descriptor.revents = 0;
      wl_display_flush(display);
      const int slice = std::min(remaining, 100);
      const int result = poll(&descriptor, 1, slice);
      remaining -= slice;
      if (result < 0 && errno == EINTR)
        continue;
      if (result < 0 ||
          (result > 0 && wl_display_dispatch(display) < 0)) {
        if (error)
          *error = "Wayland dispatch failed while awaiting configure";
        return false;
      }
      if (result == 0 && wl_display_dispatch_pending(display) < 0) {
        if (error)
          *error = "Wayland pending dispatch failed while awaiting configure";
        return false;
      }
    }
    if (!configured || (wait_for_background && !background_configured)) {
      if (error)
        *error = shutdown ? "Wayland surface was closed during configure"
                          : "timed out awaiting Wayland surface configure";
      return false;
    }
    return true;
  }

  bool create_black_buffer(std::string *error) {
    const size_t size = static_cast<size_t>(kDisplayWidth) * kDisplayHeight *
                        sizeof(uint32_t);
    const int fd = create_anonymous_file(size, error);
    if (fd < 0)
      return false;
    black_memory = static_cast<uint32_t *>(
        mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0));
    if (black_memory == MAP_FAILED) {
      black_memory = NULL;
      if (error)
        *error = errno_message("cannot map Wayland background buffer");
      ::close(fd);
      return false;
    }
    std::fill(black_memory,
              black_memory + static_cast<size_t>(kDisplayWidth) *
                                 kDisplayHeight,
              0xff000000U);
    if (std::getenv("RETRO_DECK_EXIT_HINT")) {
      const unsigned int left = 20;
      const unsigned int top = 20;
      const unsigned int cell = 4;
      for (unsigned int step = 0; step < 9; ++step) {
        for (unsigned int y = 0; y < cell; ++y) {
          for (unsigned int x = 0; x < cell; ++x) {
            const unsigned int first_x = left + step * cell + x;
            const unsigned int second_x = left + (8 - step) * cell + x;
            const unsigned int target_y = top + step * cell + y;
            black_memory[static_cast<size_t>(target_y) * kDisplayWidth +
                         first_x] = 0xffffffffU;
            black_memory[static_cast<size_t>(target_y) * kDisplayWidth +
                         second_x] = 0xffffffffU;
          }
        }
      }
    }
    wl_shm_pool *pool = wl_shm_create_pool(shm, fd, static_cast<int32_t>(size));
    black_buffer = wl_shm_pool_create_buffer(
        pool, 0, kDisplayWidth, kDisplayHeight,
        kDisplayWidth * sizeof(uint32_t), WL_SHM_FORMAT_XRGB8888);
    wl_shm_pool_destroy(pool);
    ::close(fd);
    wl_surface_attach(background_surface, black_buffer, 0, 0);
    wl_surface_damage(background_surface, 0, 0, INT_MAX, INT_MAX);
    wl_surface_commit(background_surface);
    return wl_display_flush(display) >= 0;
  }

  bool ensure_slots(unsigned int requested_width,
                    unsigned int requested_height, std::string *error) {
    if (!slots.empty()) {
      const size_t expected = static_cast<size_t>(requested_width) *
                              requested_height * sizeof(uint32_t);
      if (slots[0].size == expected)
        return true;
      for (size_t index = 0; index < slots.size(); ++index) {
        if (slots[index].busy) {
          if (error)
            *error = "Wayland buffer size changed while buffers are in use";
          return false;
        }
      }
      destroy_slots();
    }
    if (requested_width == 0 || requested_height == 0 ||
        requested_width > INT_MAX / 4 ||
        requested_height > static_cast<unsigned int>(INT_MAX) ||
        static_cast<size_t>(requested_width) >
            SIZE_MAX / sizeof(uint32_t) / requested_height) {
      if (error)
        *error = "Wayland buffer dimensions are invalid";
      return false;
    }
    const size_t size = static_cast<size_t>(requested_width) *
                        requested_height * sizeof(uint32_t);
    slots.resize(kBufferCount);
    for (size_t index = 0; index < slots.size(); ++index) {
      BufferSlot &slot = slots[index];
      slot.owner = this;
      slot.size = size;
      const int fd = create_anonymous_file(size, error);
      if (fd < 0) {
        destroy_slots();
        return false;
      }
      slot.memory = static_cast<uint32_t *>(
          mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0));
      if (slot.memory == MAP_FAILED) {
        slot.memory = NULL;
        if (error)
          *error = errno_message("cannot map Wayland frame buffer");
        ::close(fd);
        destroy_slots();
        return false;
      }
      wl_shm_pool *pool =
          wl_shm_create_pool(shm, fd, static_cast<int32_t>(size));
      slot.buffer = wl_shm_pool_create_buffer(
          pool, 0, static_cast<int32_t>(requested_width),
          static_cast<int32_t>(requested_height),
          static_cast<int32_t>(requested_width * sizeof(uint32_t)),
          WL_SHM_FORMAT_XRGB8888);
      wl_buffer_add_listener(slot.buffer, &buffer_listener, &slot);
      wl_shm_pool_destroy(pool);
      ::close(fd);
    }
    return true;
  }

  void destroy_slots() {
    for (size_t index = 0; index < slots.size(); ++index) {
      if (slots[index].buffer)
        wl_buffer_destroy(slots[index].buffer);
      if (slots[index].memory)
        munmap(slots[index].memory, slots[index].size);
    }
    slots.clear();
  }

  BufferSlot *available_slot() {
    wl_display_dispatch_pending(display);
    struct pollfd descriptor;
    descriptor.fd = wl_display_get_fd(display);
    descriptor.events = POLLIN;
    descriptor.revents = 0;
    if (poll(&descriptor, 1, 0) > 0 && (descriptor.revents & POLLIN))
      wl_display_dispatch(display);
    for (size_t index = 0; index < slots.size(); ++index) {
      if (!slots[index].busy)
        return &slots[index];
    }
    return NULL;
  }

  template <typename Reader>
  bool present(unsigned int source_width, unsigned int source_height,
               const Reader &read_pixel, std::string *error) {
    if (!configured || !surface) {
      if (error)
        *error = "Wayland surface is not configured";
      return false;
    }
    const unsigned int target_width = widget ? width : source_width;
    const unsigned int target_height = widget ? height : source_height;
    if (!ensure_slots(target_width, target_height, error))
      return false;
    BufferSlot *slot = available_slot();
    if (!slot)
      return true;
    for (unsigned int y = 0; y < target_height; ++y) {
      const unsigned int source_y =
          static_cast<unsigned int>((static_cast<uint64_t>(y) *
                                     source_height) /
                                    target_height);
      for (unsigned int x = 0; x < target_width; ++x) {
        const unsigned int source_x =
            static_cast<unsigned int>((static_cast<uint64_t>(x) *
                                       source_width) /
                                      target_width);
        slot->memory[static_cast<size_t>(y) * target_width + x] =
            read_pixel(source_x, source_y);
      }
    }
    slot->busy = true;
    wl_surface_attach(surface, slot->buffer, 0, 0);
    wl_surface_damage(surface, 0, 0, INT_MAX, INT_MAX);
    wl_surface_commit(surface);
    if (wl_display_flush(display) < 0 && errno != EAGAIN) {
      slot->busy = false;
      if (error)
        *error = "cannot flush Wayland frame";
      return false;
    }
    return true;
  }

  void close_all() {
    if (!display)
      return;
    destroy_slots();
    if (black_buffer)
      wl_buffer_destroy(black_buffer);
    black_buffer = NULL;
    if (black_memory)
      munmap(black_memory, static_cast<size_t>(kDisplayWidth) *
                                  kDisplayHeight * sizeof(uint32_t));
    black_memory = NULL;
    if (game_layer)
      zwlr_layer_surface_v1_destroy(game_layer);
    if (background_layer)
      zwlr_layer_surface_v1_destroy(background_layer);
    if (widget_surface)
      deck_widget_surface_v1_destroy(widget_surface);
    if (surface)
      wl_surface_destroy(surface);
    if (background_surface)
      wl_surface_destroy(background_surface);
    if (touch)
      wl_touch_destroy(touch);
    if (seat)
      wl_seat_destroy(seat);
    if (layer_shell)
      zwlr_layer_shell_v1_destroy(layer_shell);
    if (widget_manager)
      deck_widget_manager_v1_destroy(widget_manager);
    if (shm)
      wl_shm_destroy(shm);
    if (compositor)
      wl_compositor_destroy(compositor);
    if (registry)
      wl_registry_destroy(registry);
    wl_display_flush(display);
    wl_display_disconnect(display);
    display = NULL;
  }

  static const wl_registry_listener registry_listener;
  static const wl_seat_listener seat_listener;
  static const wl_touch_listener touch_listener;
  static const deck_widget_surface_v1_listener widget_listener;
  static const zwlr_layer_surface_v1_listener layer_listener;
  static const wl_buffer_listener buffer_listener;
};

const wl_registry_listener DeckWaylandPresentation::Impl::registry_listener = {
    registry_global, registry_remove};
const wl_seat_listener DeckWaylandPresentation::Impl::seat_listener = {
    seat_capabilities, seat_name};
const wl_touch_listener DeckWaylandPresentation::Impl::touch_listener = {
    touch_down_event,       touch_up_event,    touch_motion_event,
    touch_frame_event,      touch_cancel_event, touch_shape_event,
    touch_orientation_event};
const deck_widget_surface_v1_listener
    DeckWaylandPresentation::Impl::widget_listener = {
        widget_configure,   widget_display_info, widget_params,
        widget_configure_done, widget_string,       widget_uint,
        widget_uint,        widget_uint,         widget_uint,
        widget_uint,        widget_uint,         widget_uint,
        widget_alarm,       widget_lifecycle,    widget_shutdown,
        widget_transition,  widget_led_status};
const zwlr_layer_surface_v1_listener
    DeckWaylandPresentation::Impl::layer_listener = {layer_configure,
                                                     layer_closed};
const wl_buffer_listener DeckWaylandPresentation::Impl::buffer_listener = {
    buffer_release};

DeckWaylandPresentation::DeckWaylandPresentation() : impl_(new Impl) {}

DeckWaylandPresentation::~DeckWaylandPresentation() {
  close();
  delete impl_;
}

bool DeckWaylandPresentation::open_widget(std::string *error) {
  close();
  impl_->widget = true;
  if (!impl_->bind_globals(error)) {
    close();
    return false;
  }
  if (!impl_->widget_manager) {
    if (error)
      *error = "Wayland compositor does not provide deck_widget_manager_v1";
    close();
    return false;
  }
  impl_->surface = wl_compositor_create_surface(impl_->compositor);
  impl_->widget_surface = deck_widget_manager_v1_get_widget_surface(
      impl_->widget_manager, impl_->surface);
  deck_widget_surface_v1_add_listener(impl_->widget_surface,
                                      &Impl::widget_listener, impl_);
  wl_surface_commit(impl_->surface);
  if (!impl_->wait_until_configured(false, error)) {
    close();
    return false;
  }
  return true;
}

bool DeckWaylandPresentation::open_gameplay(unsigned int source_width,
                                            unsigned int source_height,
                                            std::string *error) {
  close();
  if (source_width == 0 || source_height == 0) {
    if (error)
      *error = "gameplay surface source dimensions are invalid";
    return false;
  }
  if (!impl_->bind_globals(error)) {
    close();
    return false;
  }
  if (!impl_->layer_shell) {
    if (error)
      *error = "Wayland compositor does not provide zwlr_layer_shell_v1";
    close();
    return false;
  }

  impl_->background_surface =
      wl_compositor_create_surface(impl_->compositor);
  wl_region *empty = wl_compositor_create_region(impl_->compositor);
  wl_surface_set_input_region(impl_->background_surface, empty);
  impl_->background_layer = zwlr_layer_shell_v1_get_layer_surface(
      impl_->layer_shell, impl_->background_surface, NULL,
      ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY, "retro-deck-game-background");
  zwlr_layer_surface_v1_add_listener(impl_->background_layer,
                                     &Impl::layer_listener, impl_);
  zwlr_layer_surface_v1_set_anchor(
      impl_->background_layer,
      ZWLR_LAYER_SURFACE_V1_ANCHOR_TOP |
          ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM |
          ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT |
          ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT);
  zwlr_layer_surface_v1_set_size(impl_->background_layer, 0, 0);
  zwlr_layer_surface_v1_set_exclusive_zone(impl_->background_layer, -1);
  zwlr_layer_surface_v1_set_keyboard_interactivity(
      impl_->background_layer,
      ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE);
  wl_surface_commit(impl_->background_surface);

  impl_->surface = wl_compositor_create_surface(impl_->compositor);
  wl_surface_set_input_region(impl_->surface, empty);
  wl_region_destroy(empty);
  impl_->game_layer = zwlr_layer_shell_v1_get_layer_surface(
      impl_->layer_shell, impl_->surface, NULL,
      ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY, "retro-deck-game");
  zwlr_layer_surface_v1_add_listener(impl_->game_layer, &Impl::layer_listener,
                                     impl_);
  compute_game_size(source_width, source_height, &impl_->width, &impl_->height);
  zwlr_layer_surface_v1_set_size(impl_->game_layer, impl_->width,
                                 impl_->height);
  zwlr_layer_surface_v1_set_keyboard_interactivity(
      impl_->game_layer, ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE);
  wl_surface_commit(impl_->surface);
  if (!impl_->wait_until_configured(true, error) ||
      !impl_->create_black_buffer(error)) {
    close();
    return false;
  }
  return true;
}

void DeckWaylandPresentation::close() {
  impl_->close_all();
  delete impl_;
  impl_ = new Impl;
}

bool DeckWaylandPresentation::is_open() const {
  return impl_->display != NULL;
}

bool DeckWaylandPresentation::is_widget() const { return impl_->widget; }

bool DeckWaylandPresentation::visible() const {
  return impl_->lifecycle_visible;
}

bool DeckWaylandPresentation::shutdown_requested() const {
  return impl_->shutdown;
}

int DeckWaylandPresentation::fd() const {
  return impl_->display ? wl_display_get_fd(impl_->display) : -1;
}

bool DeckWaylandPresentation::dispatch(std::string *error) {
  if (!impl_->display)
    return false;
  if (wl_display_dispatch(impl_->display) < 0) {
    if (error)
      *error = "Wayland display disconnected";
    return false;
  }
  return true;
}

bool DeckWaylandPresentation::dispatch_pending(std::string *error) {
  if (!impl_->display)
    return false;
  if (wl_display_dispatch_pending(impl_->display) < 0 ||
      (wl_display_flush(impl_->display) < 0 && errno != EAGAIN)) {
    if (error)
      *error = "Wayland display dispatch failed";
    return false;
  }
  return true;
}

void DeckWaylandPresentation::take_touch_reports(
    std::vector<DeckWaylandTouchReport> *reports) {
  if (!reports)
    return;
  reports->swap(impl_->touch_reports);
  impl_->touch_reports.clear();
}

bool DeckWaylandPresentation::present_rgb565(const void *pixels,
                                             unsigned int width,
                                             unsigned int height,
                                             size_t pitch,
                                             std::string *error) {
  if (!pixels || pitch < static_cast<size_t>(width) * sizeof(uint16_t)) {
    if (error)
      *error = "invalid RGB565 frame";
    return false;
  }
  const uint8_t *bytes = static_cast<const uint8_t *>(pixels);
  return impl_->present(width, height,
                        [bytes, pitch](unsigned int x, unsigned int y) {
                          const uint16_t *row = reinterpret_cast<const uint16_t *>(
                              bytes + static_cast<size_t>(y) * pitch);
                          return rgb565_to_xrgb(row[x]);
                        },
                        error);
}

bool DeckWaylandPresentation::present_xrgb8888(const void *pixels,
                                               unsigned int width,
                                               unsigned int height,
                                               size_t pitch,
                                               std::string *error) {
  if (!pixels || pitch < static_cast<size_t>(width) * sizeof(uint32_t)) {
    if (error)
      *error = "invalid XRGB8888 frame";
    return false;
  }
  const uint8_t *bytes = static_cast<const uint8_t *>(pixels);
  return impl_->present(width, height,
                        [bytes, pitch](unsigned int x, unsigned int y) {
                          const uint32_t *row = reinterpret_cast<const uint32_t *>(
                              bytes + static_cast<size_t>(y) * pitch);
                          return 0xff000000U | (row[x] & 0x00ffffffU);
                        },
                        error);
}

bool DeckWaylandPresentation::present_indexed(
    const uint8_t *pixels, unsigned int width, unsigned int height,
    size_t pitch, const uint32_t *palette, size_t palette_size,
    std::string *error) {
  if (!pixels || pitch < width || !palette || palette_size == 0) {
    if (error)
      *error = "invalid indexed frame";
    return false;
  }
  return impl_->present(
      width, height,
      [pixels, pitch, palette, palette_size](unsigned int x, unsigned int y) {
        const uint8_t index = pixels[static_cast<size_t>(y) * pitch + x];
        const uint32_t color = index < palette_size ? palette[index] : 0;
        return 0xff000000U | (color & 0x00ffffffU);
      },
      error);
}
