#ifndef RETRO_DECK_WAYLAND_H
#define RETRO_DECK_WAYLAND_H

#include <stddef.h>
#include <stdint.h>

#include <string>
#include <vector>

struct DeckWaylandTouchReport {
  int x;
  int y;
  bool down;
  bool pressed;
  bool released;
};

class DeckWaylandPresentation {
public:
  DeckWaylandPresentation();
  ~DeckWaylandPresentation();

  bool open_widget(std::string *error);
  bool open_gameplay(unsigned int source_width, unsigned int source_height,
                     std::string *error);
  void close();

  bool is_open() const;
  bool is_widget() const;
  bool visible() const;
  bool shutdown_requested() const;
  int fd() const;
  bool dispatch(std::string *error);
  bool dispatch_pending(std::string *error);
  void take_touch_reports(std::vector<DeckWaylandTouchReport> *reports);

  bool present_rgb565(const void *pixels, unsigned int width,
                      unsigned int height, size_t pitch, std::string *error);
  bool present_xrgb8888(const void *pixels, unsigned int width,
                        unsigned int height, size_t pitch,
                        std::string *error);
  bool present_indexed(const uint8_t *pixels, unsigned int width,
                       unsigned int height, size_t pitch,
                       const uint32_t *palette, size_t palette_size,
                       std::string *error);

private:
  DeckWaylandPresentation(const DeckWaylandPresentation &);
  DeckWaylandPresentation &operator=(const DeckWaylandPresentation &);

  struct Impl;
  Impl *impl_;
};

#endif /* RETRO_DECK_WAYLAND_H */
