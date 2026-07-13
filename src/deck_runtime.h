#ifndef RETRO_DECK_RUNTIME_H
#define RETRO_DECK_RUNTIME_H

#include <stddef.h>
#include <stdint.h>

#include <string>
#include <vector>

/*
 * Shared, dependency-free runtime for the Deck's framebuffer emulators.
 * The LCD is physically portrait but is exposed to games as a 1280x480
 * logical surface.  The 16-pixel safe area keeps content clear of the
 * rounded display corners.
 */

struct DeckScaledLayout {
  int scale;
  int x;
  int y;
  int width;
  int height;
};

bool DeckComputeScaledLayout(unsigned int source_width,
                             unsigned int source_height,
                             DeckScaledLayout *layout);
uint16_t DeckRgb888To565(uint32_t color);
bool DeckReadVolumePercent(unsigned int *volume, std::string *error);

class DeckFramebuffer {
public:
  DeckFramebuffer();
  ~DeckFramebuffer();

  bool open_device(std::string *error);
  void close_device();
  bool present_xrgb8888(const void *pixels, unsigned int width,
                        unsigned int height, size_t pitch,
                        std::string *error);
  bool present_rgb565(const void *pixels, unsigned int width,
                      unsigned int height, size_t pitch,
                      std::string *error);
  bool present_indexed(const uint8_t *pixels, unsigned int width,
                       unsigned int height, size_t pitch,
                       const uint32_t *palette, size_t palette_size,
                       std::string *error);

private:
  bool begin_frame(unsigned int width, unsigned int height,
                   DeckScaledLayout *layout, std::string *error);
  void draw_scaled_pixel(const DeckScaledLayout &layout,
                         unsigned int source_x, unsigned int source_y,
                         uint16_t color);
  void publish_frame(const DeckScaledLayout &layout);

  int fd_;
  unsigned char *memory_;
  size_t map_size_;
  int stride_;
  unsigned int last_source_width_;
  unsigned int last_source_height_;
  std::vector<uint16_t> frame_;
};

class DeckAudio {
public:
  DeckAudio();
  ~DeckAudio();

  /* Audio failure is non-fatal to emulation; callers can use FrameClock. */
  bool open_device(unsigned int source_rate, unsigned int volume_percent,
                   std::string *error);
  void close_device();
  bool available() const;
  bool write_stereo(const int16_t *samples, size_t frames);
  bool write_mono(const int16_t *samples, size_t frames);
  bool write_square_frame(bool active);

private:
  bool start_playback();
  bool write_all(const int16_t *samples, size_t count);

  int fd_;
  unsigned int source_rate_;
  unsigned int device_rate_;
  unsigned int volume_percent_;
  uint64_t rate_remainder_;
  uint32_t square_phase_;
  bool trigger_pending_;
  std::vector<int16_t> mono_buffer_;
  std::vector<int16_t> resample_buffer_;
};

class DeckFrameClock {
public:
  explicit DeckFrameClock(double frames_per_second);
  void wait_for_next_frame();

private:
  int64_t start_nanoseconds_;
  int64_t frame_nanoseconds_;
  uint64_t frame_number_;
};

#endif /* RETRO_DECK_RUNTIME_H */
