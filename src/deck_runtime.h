#ifndef RETRO_DECK_RUNTIME_H
#define RETRO_DECK_RUNTIME_H

#include <stddef.h>
#include <stdint.h>
#include <pthread.h>

#include <string>
#include <vector>

#ifdef RETRO_DECK_WAYLAND
class DeckWaylandPresentation;
#endif

/*
 * Shared runtime for the Deck's framebuffer and Wayland emulator frontends.
 * Direct fbdev scanout is physically portrait; Wayland exposes the 1280x480
 * logical display. The 16-pixel safe area keeps content clear of the rounded
 * display corners.
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
unsigned int DeckAudioOutputRate(unsigned int source_rate,
                                 unsigned int negotiated_rate);
bool DeckExitHintRequested();

inline void DeckDrawExitHintRgb565(uint16_t *pixels, size_t row_stride) {
  if (!pixels || row_stride < 600)
    return;
  const int left = 20;
  const int top = 20;
  const int cell = 4;
  const int physical_height = 1280;
  const auto fill_logical_rect =
      [pixels, row_stride, physical_height](int x, int y, int width,
                                             int height, uint16_t color) {
        for (int logical_x = x; logical_x < x + width; ++logical_x) {
          if (logical_x < 0 || logical_x >= physical_height)
            continue;
          const size_t physical_row =
              static_cast<size_t>(physical_height - 1 - logical_x);
          for (int logical_y = y; logical_y < y + height; ++logical_y) {
            if (logical_y >= 0 && logical_y < 480)
              pixels[physical_row * row_stride +
                     static_cast<size_t>(logical_y)] = color;
          }
        }
      };
  for (int step = 0; step < 9; ++step) {
    const int x = left + step * cell;
    const int opposite_x = left + (8 - step) * cell;
    const int y = top + step * cell;
    fill_logical_rect(x - 2, y - 2, cell + 4, cell + 4, 0x0000);
    fill_logical_rect(opposite_x - 2, y - 2, cell + 4, cell + 4, 0x0000);
  }
  for (int step = 0; step < 9; ++step) {
    const int x = left + step * cell;
    const int opposite_x = left + (8 - step) * cell;
    const int y = top + step * cell;
    fill_logical_rect(x, y, cell, cell, 0xffff);
    fill_logical_rect(opposite_x, y, cell, cell, 0xffff);
  }
}

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
  bool exit_hint_;
  std::vector<uint16_t> frame_;
#ifdef RETRO_DECK_WAYLAND
  DeckWaylandPresentation *wayland_;
#endif
};

class DeckAudio {
public:
  DeckAudio();
  ~DeckAudio();

  /* Audio failure is non-fatal to emulation; callers can use FrameClock. */
  bool open_device(unsigned int source_rate, unsigned int volume_percent,
                   std::string *error, unsigned int fragment_count = 8);
  void close_device();
  bool available() const;
  size_t queued_frames() const;
  uint64_t dropped_frames() const;
  bool write_stereo(const int16_t *samples, size_t frames);
  bool write_mono(const int16_t *samples, size_t frames);
  bool write_square_frame(bool active);

private:
  static void *audio_thread_entry(void *context);
  void audio_thread_main();
  bool enqueue(const int16_t *samples, size_t count);
  bool start_playback();
  bool write_all(const int16_t *samples, size_t count);

  int fd_;
  unsigned int source_rate_;
  unsigned int output_rate_;
  unsigned int volume_percent_;
  uint64_t rate_remainder_;
  uint32_t square_phase_;
  bool trigger_pending_;
  pthread_t thread_;
  mutable pthread_mutex_t mutex_;
  pthread_cond_t condition_;
  bool thread_started_;
  bool stopping_;
  bool worker_failed_;
  size_t queue_head_;
  size_t queue_size_;
  uint64_t dropped_frames_;
  std::vector<int16_t> mono_buffer_;
  std::vector<int16_t> resample_buffer_;
  std::vector<int16_t> queue_;
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
