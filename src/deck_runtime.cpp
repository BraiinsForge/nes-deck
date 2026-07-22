#include "deck_runtime.h"

#ifdef RETRO_DECK_WAYLAND
#include "deck_wayland.h"
#endif

#include <algorithm>
#include <cerrno>
#include <climits>
#include <cmath>
#include <cstring>
#include <fcntl.h>
#include <linux/fb.h>
#include <linux/soundcard.h>
#include <limits>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

namespace {

const int kLogicalWidth = 1280;
const int kLogicalHeight = 480;
const int kPhysicalWidth = 600;
const int kPhysicalHeight = 1280;
const int kSafeInset = 16;
const size_t kAudioQueueFrames = 16384;
const size_t kAudioWriteChunkFrames = 2048;

std::string system_error(const std::string &what) {
  return what + ": " + std::strerror(errno);
}

int64_t monotonic_nanoseconds() {
  struct timespec now;
  if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
    return 0;
  return static_cast<int64_t>(now.tv_sec) * 1000000000LL + now.tv_nsec;
}

int16_t scale_sample(int16_t sample, unsigned int percent) {
  int32_t scaled = static_cast<int32_t>(sample) *
                   static_cast<int32_t>(percent) / 100;
  if (scaled < -32768)
    scaled = -32768;
  if (scaled > 32767)
    scaled = 32767;
  return static_cast<int16_t>(scaled);
}

} // namespace

bool DeckComputeScaledLayout(unsigned int source_width,
                             unsigned int source_height,
                             DeckScaledLayout *layout) {
  if (!layout || source_width == 0 || source_height == 0)
    return false;
  const int usable_width = kLogicalWidth - 2 * kSafeInset;
  const int usable_height = kLogicalHeight - 2 * kSafeInset;
  if (source_width > static_cast<unsigned int>(usable_width) ||
      source_height > static_cast<unsigned int>(usable_height))
    return false;

  const int horizontal = usable_width / static_cast<int>(source_width);
  const int vertical = usable_height / static_cast<int>(source_height);
  const int scale = std::min(horizontal, vertical);
  if (scale < 1)
    return false;

  layout->scale = scale;
  layout->width = static_cast<int>(source_width) * scale;
  layout->height = static_cast<int>(source_height) * scale;
  layout->x = (kLogicalWidth - layout->width) / 2;
  layout->y = (kLogicalHeight - layout->height) / 2;
  return layout->x >= kSafeInset && layout->y >= kSafeInset &&
         layout->x + layout->width <= kLogicalWidth - kSafeInset &&
         layout->y + layout->height <= kLogicalHeight - kSafeInset;
}

uint16_t DeckRgb888To565(uint32_t color) {
  const unsigned int red = (color >> 16) & 0xff;
  const unsigned int green = (color >> 8) & 0xff;
  const unsigned int blue = color & 0xff;
  return static_cast<uint16_t>(((red & 0xf8) << 8) |
                               ((green & 0xfc) << 3) | (blue >> 3));
}

bool DeckReadVolumePercent(unsigned int *volume, std::string *error) {
  if (!volume)
    return false;
  *volume = 42;
  const char *text = std::getenv("RETRO_DECK_VOLUME_PERCENT");
  if (!text)
    return true;
  if (!*text) {
    if (error)
      *error = "volume must be an integer from 0 through 100";
    return false;
  }
  unsigned int parsed = 0;
  for (const char *cursor = text; *cursor; ++cursor) {
    if (*cursor < '0' || *cursor > '9') {
      if (error)
        *error = "volume must be an integer from 0 through 100";
      return false;
    }
    parsed = parsed * 10 + static_cast<unsigned int>(*cursor - '0');
    if (parsed > 100) {
      if (error)
        *error = "volume must be an integer from 0 through 100";
      return false;
    }
  }
  *volume = parsed;
  return true;
}

unsigned int DeckAudioOutputRate(unsigned int source_rate,
                                 unsigned int negotiated_rate) {
  // The Deck's OSS bridge reports an exact 48 kHz stream, but live queue and
  // frame-clock measurements show that it consumes about 47,328 mono
  // application frames per second.  Sending all 48,000 blocks the emulator
  // about 1.5 percent slow.  Resample to the measured application clock while
  // leaving ALSA configured at its required nominal 48 kHz rate.
  if (source_rate == 48000 && negotiated_rate == 48000)
    return 47328;
  return negotiated_rate;
}

bool DeckExitHintRequested() {
  const char *value = std::getenv("RETRO_DECK_EXIT_HINT");
  return value && std::strcmp(value, "1") == 0;
}

DeckFramebuffer::DeckFramebuffer()
    : fd_(-1), memory_(NULL), map_size_(0), stride_(0),
      last_source_width_(0), last_source_height_(0),
      exit_hint_(DeckExitHintRequested())
#ifdef RETRO_DECK_WAYLAND
      , wayland_(NULL)
#endif
{}

DeckFramebuffer::~DeckFramebuffer() { close_device(); }

bool DeckFramebuffer::open_device(std::string *error) {
  close_device();
#ifdef RETRO_DECK_WAYLAND
  const char *presentation = std::getenv("RETRO_DECK_PRESENTATION");
  const char *wayland_display = std::getenv("WAYLAND_DISPLAY");
  if (presentation && std::strcmp(presentation, "layer-shell") == 0 &&
      wayland_display && wayland_display[0]) {
    wayland_ = new DeckWaylandPresentation;
    return true;
  }
#endif
  fd_ = open("/dev/fb0", O_RDWR | O_CLOEXEC);
  if (fd_ < 0) {
    if (error)
      *error = system_error("cannot open /dev/fb0");
    return false;
  }

  struct fb_var_screeninfo variable;
  struct fb_fix_screeninfo fixed;
  std::memset(&variable, 0, sizeof(variable));
  std::memset(&fixed, 0, sizeof(fixed));
  if (ioctl(fd_, FBIOGET_VSCREENINFO, &variable) != 0 ||
      ioctl(fd_, FBIOGET_FSCREENINFO, &fixed) != 0) {
    if (error)
      *error = system_error("cannot query framebuffer geometry");
    close_device();
    return false;
  }

  const unsigned int rows =
      variable.yres_virtual ? variable.yres_virtual : variable.yres;
  if (fixed.line_length == 0 ||
      rows > std::numeric_limits<size_t>::max() / fixed.line_length) {
    if (error)
      *error = "framebuffer geometry overflows the address space";
    close_device();
    return false;
  }
  const size_t required = static_cast<size_t>(fixed.line_length) * rows;
  if (variable.xres != kPhysicalWidth ||
      variable.yres != kPhysicalHeight || variable.bits_per_pixel != 16 ||
      variable.xoffset != 0 || variable.yoffset != 0 ||
      rows < kPhysicalHeight || fixed.type != FB_TYPE_PACKED_PIXELS ||
      fixed.visual != FB_VISUAL_TRUECOLOR || fixed.line_length > INT_MAX ||
      fixed.line_length < kPhysicalWidth * 2 ||
      (fixed.line_length & 1) != 0 || fixed.smem_len < required ||
      variable.red.offset != 11 || variable.red.length != 5 ||
      variable.red.msb_right != 0 || variable.green.offset != 5 ||
      variable.green.length != 6 || variable.green.msb_right != 0 ||
      variable.blue.offset != 0 || variable.blue.length != 5 ||
      variable.blue.msb_right != 0 || variable.transp.length != 0) {
    if (error)
      *error = "unsupported framebuffer; expected 600x1280 RGB565 with a "
               "valid stride";
    close_device();
    return false;
  }

  stride_ = static_cast<int>(fixed.line_length);
  map_size_ = fixed.smem_len;
  memory_ = static_cast<unsigned char *>(
      mmap(NULL, map_size_, PROT_READ | PROT_WRITE, MAP_SHARED, fd_, 0));
  if (memory_ == MAP_FAILED) {
    memory_ = NULL;
    if (error)
      *error = system_error("cannot mmap /dev/fb0");
    close_device();
    return false;
  }
  frame_.assign(map_size_ / sizeof(uint16_t), 0);
  std::memset(memory_, 0, map_size_);
  return true;
}

void DeckFramebuffer::close_device() {
#ifdef RETRO_DECK_WAYLAND
  delete wayland_;
  wayland_ = NULL;
#endif
  if (memory_) {
    munmap(memory_, map_size_);
    memory_ = NULL;
  }
  if (fd_ >= 0) {
    close(fd_);
    fd_ = -1;
  }
  map_size_ = 0;
  stride_ = 0;
  last_source_width_ = 0;
  last_source_height_ = 0;
  frame_.clear();
}

bool DeckFramebuffer::begin_frame(unsigned int width, unsigned int height,
                                  DeckScaledLayout *layout,
                                  std::string *error) {
  if (!memory_) {
    if (error)
      *error = "framebuffer is not open";
    return false;
  }
  if (!DeckComputeScaledLayout(width, height, layout)) {
    if (error)
      *error = "video frame does not fit the Deck safe area";
    return false;
  }
  if (frame_.size() * sizeof(uint16_t) != map_size_) {
    if (error)
      *error = "framebuffer staging buffer is unavailable";
    return false;
  }
  if (width != last_source_width_ || height != last_source_height_) {
    std::fill(frame_.begin(), frame_.end(), 0);
    std::memset(memory_, 0, map_size_);
    last_source_width_ = width;
    last_source_height_ = height;
  }
  return true;
}

void DeckFramebuffer::draw_scaled_pixel(const DeckScaledLayout &layout,
                                        unsigned int source_x,
                                        unsigned int source_y,
                                        uint16_t color) {
  const int left = layout.x + static_cast<int>(source_x) * layout.scale;
  const int top = layout.y + static_cast<int>(source_y) * layout.scale;
  for (int y = 0; y < layout.scale; ++y) {
    const int physical_column = top + y;
    for (int x = 0; x < layout.scale; ++x) {
      const int logical_x = left + x;
      const int physical_row = kPhysicalHeight - 1 - logical_x;
      uint16_t *destination =
          &frame_[static_cast<size_t>(physical_row) *
                  (static_cast<size_t>(stride_) / sizeof(uint16_t))];
      destination[physical_column] = color;
    }
  }
}

void DeckFramebuffer::publish_frame(const DeckScaledLayout &layout) {
  // The panel scans memory while userspace writes it.  Build the rotated,
  // scaled image in cacheable RAM first, then publish only its completed
  // rectangle.  Avoiding the untouched black margins cuts GB scanout traffic
  // from 1.64 MB to about 0.41 MB per frame and leaves time for audio.
  const size_t row_words =
      static_cast<size_t>(stride_) / sizeof(uint16_t);
  const size_t copy_bytes =
      static_cast<size_t>(layout.height) * sizeof(uint16_t);
  const int first_physical_row =
      kPhysicalHeight - layout.x - layout.width;
  for (int row = 0; row < layout.width; ++row) {
    const size_t offset =
        static_cast<size_t>(first_physical_row + row) * row_words +
        static_cast<size_t>(layout.y);
    std::memcpy(memory_ + offset * sizeof(uint16_t), &frame_[offset],
                copy_bytes);
  }
  if (exit_hint_)
    DeckDrawExitHintRgb565(reinterpret_cast<uint16_t *>(memory_), row_words);
}

bool DeckFramebuffer::present_xrgb8888(const void *pixels,
                                       unsigned int width,
                                       unsigned int height, size_t pitch,
                                       std::string *error) {
#ifdef RETRO_DECK_WAYLAND
  if (wayland_) {
    if (!wayland_->is_open() && !wayland_->open_gameplay(width, height, error))
      return false;
    return wayland_->present_xrgb8888(pixels, width, height, pitch, error);
  }
#endif
  if (!pixels || pitch < static_cast<size_t>(width) * sizeof(uint32_t)) {
    if (error)
      *error = "invalid XRGB8888 video frame";
    return false;
  }
  DeckScaledLayout layout;
  if (!begin_frame(width, height, &layout, error))
    return false;

  const unsigned char *rows = static_cast<const unsigned char *>(pixels);
  for (unsigned int y = 0; y < height; ++y) {
    const uint32_t *row =
        reinterpret_cast<const uint32_t *>(rows + static_cast<size_t>(y) * pitch);
    for (unsigned int x = 0; x < width; ++x)
      draw_scaled_pixel(layout, x, y, DeckRgb888To565(row[x]));
  }
  publish_frame(layout);
  return true;
}

bool DeckFramebuffer::present_rgb565(const void *pixels, unsigned int width,
                                     unsigned int height, size_t pitch,
                                     std::string *error) {
#ifdef RETRO_DECK_WAYLAND
  if (wayland_) {
    if (!wayland_->is_open() && !wayland_->open_gameplay(width, height, error))
      return false;
    return wayland_->present_rgb565(pixels, width, height, pitch, error);
  }
#endif
  if (!pixels || pitch < static_cast<size_t>(width) * sizeof(uint16_t)) {
    if (error)
      *error = "invalid RGB565 video frame";
    return false;
  }
  DeckScaledLayout layout;
  if (!begin_frame(width, height, &layout, error))
    return false;

  const unsigned char *rows = static_cast<const unsigned char *>(pixels);
  const size_t destination_stride =
      static_cast<size_t>(stride_) / sizeof(uint16_t);
  // The logical display is rotated onto the portrait framebuffer.  Build one
  // complete physical row at a time so writes stay sequential in cache.  The
  // generic pixel helper instead bounced among hundreds of physical rows for
  // every source scanline, which cost the Cortex-A7 several FPS.
  for (unsigned int source_x = 0; source_x < width; ++source_x) {
    const int first_physical_row =
        kPhysicalHeight - 1 - layout.x -
        static_cast<int>(source_x) * layout.scale;
    for (int duplicate_x = 0; duplicate_x < layout.scale; ++duplicate_x) {
      uint16_t *destination =
          &frame_[static_cast<size_t>(first_physical_row - duplicate_x) *
                      destination_stride +
                  static_cast<size_t>(layout.y)];
      for (unsigned int source_y = 0; source_y < height; ++source_y) {
        const uint16_t *source_row = reinterpret_cast<const uint16_t *>(
            rows + static_cast<size_t>(source_y) * pitch);
        const uint16_t color = source_row[source_x];
        for (int duplicate_y = 0; duplicate_y < layout.scale; ++duplicate_y)
          *destination++ = color;
      }
    }
  }
  publish_frame(layout);
  return true;
}

bool DeckFramebuffer::present_indexed(const uint8_t *pixels,
                                      unsigned int width,
                                      unsigned int height, size_t pitch,
                                      const uint32_t *palette,
                                      size_t palette_size,
                                      std::string *error) {
#ifdef RETRO_DECK_WAYLAND
  if (wayland_) {
    if (!wayland_->is_open() && !wayland_->open_gameplay(width, height, error))
      return false;
    return wayland_->present_indexed(pixels, width, height, pitch, palette,
                                     palette_size, error);
  }
#endif
  if (!pixels || !palette || palette_size == 0 || pitch < width) {
    if (error)
      *error = "invalid indexed video frame";
    return false;
  }
  DeckScaledLayout layout;
  if (!begin_frame(width, height, &layout, error))
    return false;

  for (unsigned int y = 0; y < height; ++y) {
    const uint8_t *row = pixels + static_cast<size_t>(y) * pitch;
    for (unsigned int x = 0; x < width; ++x) {
      const size_t index = row[x];
      const uint32_t color = index < palette_size ? palette[index] : 0;
      draw_scaled_pixel(layout, x, y, DeckRgb888To565(color));
    }
  }
  publish_frame(layout);
  return true;
}

DeckAudio::DeckAudio()
    : fd_(-1), source_rate_(0), output_rate_(0), volume_percent_(0),
      rate_remainder_(0), square_phase_(0), trigger_pending_(false), thread_(),
      thread_started_(false), stopping_(false), worker_failed_(false),
      queue_head_(0), queue_size_(0), dropped_frames_(0) {
  pthread_mutex_init(&mutex_, NULL);
  pthread_cond_init(&condition_, NULL);
}

DeckAudio::~DeckAudio() {
  close_device();
  pthread_cond_destroy(&condition_);
  pthread_mutex_destroy(&mutex_);
}

bool DeckAudio::open_device(unsigned int source_rate,
                            unsigned int volume_percent,
                            std::string *error,
                            unsigned int fragment_count) {
  close_device();
  if (source_rate == 0 || volume_percent > 100 || fragment_count == 0 ||
      fragment_count > 64) {
    if (error)
      *error = "invalid audio rate, volume, or buffer size";
    return false;
  }
  // Muted playback needs no OSS device; the frame clock still paces the
  // emulator.  This also gives runtime diagnostics a clean CPU/video timing
  // path that is independent of the sound driver's negotiated rate.
  if (volume_percent == 0)
    return true;
  fd_ = open("/dev/dsp", O_WRONLY | O_CLOEXEC);
  if (fd_ < 0) {
    if (error)
      *error = system_error("cannot open /dev/dsp");
    return false;
  }

  // Use 1024-byte S16 periods.  The default eight-period ring is roughly
  // 93 ms at 44.1 kHz mono; the independently paced chiptune player requests
  // four periods for roughly 46 ms of control latency.  The earlier four
  // 512-byte periods left only about 23 ms and audibly underrran during
  // framebuffer updates.
  int fragment = (static_cast<int>(fragment_count) << 16) | 10;
  int format = AFMT_S16_LE;
  int channels = 1;
  // The Deck's OSS compatibility layer reports a requested 32768 Hz stream
  // as 32768 even though the live ALSA hardware stream runs at 32000 Hz.  It
  // then consumes one application frame per hardware frame, slowing Gambatte
  // by exactly 32768/32000.  Request the real supported rate and use our
  // explicit resampler instead.
  int rate = source_rate == 32768 ? 32000 : static_cast<int>(source_rate);
  ioctl(fd_, SNDCTL_DSP_SETFRAGMENT, &fragment);
  if (ioctl(fd_, SNDCTL_DSP_SETFMT, &format) != 0 ||
      format != AFMT_S16_LE ||
      ioctl(fd_, SNDCTL_DSP_CHANNELS, &channels) != 0 || channels != 1 ||
      ioctl(fd_, SNDCTL_DSP_SPEED, &rate) != 0 || rate <= 0) {
    const int saved_errno = errno;
    close_device();
    errno = saved_errno;
    if (error)
      *error = system_error("cannot configure /dev/dsp");
    return false;
  }

  source_rate_ = source_rate;
  output_rate_ = DeckAudioOutputRate(source_rate,
                                     static_cast<unsigned int>(rate));
  volume_percent_ = volume_percent;

  // Hold playback while the complete ring is primed.  Starting an empty OSS
  // ring on the first small emulator callback produced a repeatable startup
  // XRUN and audible corruption on GB/GBC.
  int trigger = 0;
  trigger_pending_ = ioctl(fd_, SNDCTL_DSP_SETTRIGGER, &trigger) == 0;
  audio_buf_info space;
  std::memset(&space, 0, sizeof(space));
  if (ioctl(fd_, SNDCTL_DSP_GETOSPACE, &space) == 0 && space.bytes > 0 &&
      space.bytes <= 1024 * 1024 &&
      (space.bytes % static_cast<int>(sizeof(int16_t))) == 0) {
    std::vector<int16_t> silence(
        static_cast<size_t>(space.bytes) / sizeof(int16_t), 0);
    if (!write_all(&silence[0], silence.size())) {
      if (error)
        *error = "cannot prefill /dev/dsp";
      close_device();
      return false;
    }
  }

  queue_.assign(kAudioQueueFrames, 0);
  queue_head_ = 0;
  queue_size_ = 0;
  dropped_frames_ = 0;
  stopping_ = false;
  worker_failed_ = false;
  const int thread_error =
      pthread_create(&thread_, NULL, audio_thread_entry, this);
  if (thread_error != 0) {
    if (error)
      *error = "cannot start audio writer: " +
               std::string(std::strerror(thread_error));
    close_device();
    return false;
  }
  thread_started_ = true;
  return true;
}

void DeckAudio::close_device() {
  if (thread_started_) {
    pthread_mutex_lock(&mutex_);
    stopping_ = true;
    pthread_cond_broadcast(&condition_);
    pthread_mutex_unlock(&mutex_);
    pthread_join(thread_, NULL);
    thread_started_ = false;
  }
  if (fd_ >= 0) {
    close(fd_);
    fd_ = -1;
  }
  source_rate_ = 0;
  output_rate_ = 0;
  volume_percent_ = 0;
  rate_remainder_ = 0;
  square_phase_ = 0;
  trigger_pending_ = false;
  stopping_ = false;
  worker_failed_ = false;
  queue_head_ = 0;
  queue_size_ = 0;
  dropped_frames_ = 0;
  mono_buffer_.clear();
  resample_buffer_.clear();
  queue_.clear();
}

bool DeckAudio::available() const {
  pthread_mutex_lock(&mutex_);
  const bool available = thread_started_ && !worker_failed_;
  pthread_mutex_unlock(&mutex_);
  return available;
}

size_t DeckAudio::queued_frames() const {
  pthread_mutex_lock(&mutex_);
  const size_t queued = queue_size_;
  pthread_mutex_unlock(&mutex_);
  return queued;
}

uint64_t DeckAudio::dropped_frames() const {
  pthread_mutex_lock(&mutex_);
  const uint64_t dropped = dropped_frames_;
  pthread_mutex_unlock(&mutex_);
  return dropped;
}

void *DeckAudio::audio_thread_entry(void *context) {
  DeckAudio *audio = static_cast<DeckAudio *>(context);
  if (audio)
    audio->audio_thread_main();
  return NULL;
}

void DeckAudio::audio_thread_main() {
  std::vector<int16_t> output(kAudioWriteChunkFrames);
  while (true) {
    pthread_mutex_lock(&mutex_);
    while (!stopping_ && queue_size_ == 0)
      pthread_cond_wait(&condition_, &mutex_);
    if (stopping_) {
      pthread_mutex_unlock(&mutex_);
      break;
    }

    const size_t count = std::min(queue_size_, output.size());
    const size_t first = std::min(count, queue_.size() - queue_head_);
    std::memcpy(&output[0], &queue_[queue_head_],
                first * sizeof(output[0]));
    if (count > first) {
      std::memcpy(&output[first], &queue_[0],
                  (count - first) * sizeof(output[0]));
    }
    queue_head_ = (queue_head_ + count) % queue_.size();
    queue_size_ -= count;
    pthread_mutex_unlock(&mutex_);

    if (start_playback() && write_all(&output[0], count))
      continue;
    pthread_mutex_lock(&mutex_);
    worker_failed_ = true;
    queue_size_ = 0;
    pthread_mutex_unlock(&mutex_);
    break;
  }
}

bool DeckAudio::enqueue(const int16_t *samples, size_t count) {
  if (!samples || count == 0)
    return false;
  pthread_mutex_lock(&mutex_);
  if (!thread_started_ || stopping_ || worker_failed_ || queue_.empty()) {
    pthread_mutex_unlock(&mutex_);
    return false;
  }

  if (count > queue_.size()) {
    const size_t skipped = count - queue_.size();
    samples += skipped;
    count -= skipped;
    dropped_frames_ += skipped;
  }
  if (count > queue_.size() - queue_size_) {
    const size_t discarded = count - (queue_.size() - queue_size_);
    queue_head_ = (queue_head_ + discarded) % queue_.size();
    queue_size_ -= discarded;
    dropped_frames_ += discarded;
  }

  const size_t tail = (queue_head_ + queue_size_) % queue_.size();
  const size_t first = std::min(count, queue_.size() - tail);
  std::memcpy(&queue_[tail], samples, first * sizeof(samples[0]));
  if (count > first) {
    std::memcpy(&queue_[0], samples + first,
                (count - first) * sizeof(samples[0]));
  }
  queue_size_ += count;
  pthread_cond_signal(&condition_);
  pthread_mutex_unlock(&mutex_);
  return true;
}

bool DeckAudio::start_playback() {
  if (!trigger_pending_)
    return fd_ >= 0;
  int trigger = PCM_ENABLE_OUTPUT;
  if (ioctl(fd_, SNDCTL_DSP_SETTRIGGER, &trigger) != 0) {
    close(fd_);
    fd_ = -1;
    trigger_pending_ = false;
    return false;
  }
  trigger_pending_ = false;
  return true;
}

bool DeckAudio::write_all(const int16_t *samples, size_t count) {
  const unsigned char *bytes =
      reinterpret_cast<const unsigned char *>(samples);
  size_t remaining = count * sizeof(*samples);
  while (remaining > 0) {
    const ssize_t amount = write(fd_, bytes, remaining);
    if (amount > 0) {
      bytes += amount;
      remaining -= static_cast<size_t>(amount);
    } else if (amount < 0 && errno == EINTR) {
      continue;
    } else {
      close(fd_);
      fd_ = -1;
      return false;
    }
  }
  return true;
}

bool DeckAudio::write_stereo(const int16_t *samples, size_t frames) {
  if (!available() || !samples)
    return false;
  mono_buffer_.resize(frames);
  for (size_t i = 0; i < frames; ++i) {
    const int32_t mixed =
        (static_cast<int32_t>(samples[i * 2]) + samples[i * 2 + 1]) / 2;
    mono_buffer_[i] =
        scale_sample(static_cast<int16_t>(mixed), volume_percent_);
  }
  return write_mono(&mono_buffer_[0], frames);
}

bool DeckAudio::write_mono(const int16_t *samples, size_t frames) {
  if (!samples || frames == 0)
    return false;
  if (source_rate_ == output_rate_)
    return enqueue(samples, frames);

  const uint64_t scaled = static_cast<uint64_t>(frames) * output_rate_ +
                          rate_remainder_;
  const size_t output_frames = static_cast<size_t>(scaled / source_rate_);
  rate_remainder_ = scaled % source_rate_;
  if (output_frames == 0)
    return true;
  resample_buffer_.resize(output_frames);
  if (frames == 1 || output_frames == 1) {
    std::fill(resample_buffer_.begin(), resample_buffer_.end(), samples[0]);
  } else {
    const uint64_t step =
        (static_cast<uint64_t>(frames - 1) << 32) / (output_frames - 1);
    uint64_t position = 0;
    for (size_t i = 0; i < output_frames; ++i) {
      const size_t first = static_cast<size_t>(position >> 32);
      const uint32_t fraction =
          static_cast<uint32_t>((position >> 16) & 0xffff);
      if (first >= frames - 1) {
        resample_buffer_[i] = samples[frames - 1];
      } else {
        const int value = samples[first] +
                          ((static_cast<int>(samples[first + 1]) -
                            samples[first]) *
                           static_cast<int>(fraction) >> 16);
        resample_buffer_[i] = static_cast<int16_t>(value);
      }
      position += step;
    }
  }
  return enqueue(&resample_buffer_[0], output_frames);
}

bool DeckAudio::write_square_frame(bool active) {
  if (!available() || source_rate_ == 0)
    return false;
  const size_t frames = source_rate_ / 60;
  mono_buffer_.resize(frames);
  const uint32_t period = std::max(2U, source_rate_ / 440U);
  for (size_t i = 0; i < frames; ++i) {
    int16_t sample = 0;
    if (active) {
      const int16_t raw = square_phase_ < period / 2 ? 6000 : -6000;
      sample = scale_sample(raw, volume_percent_);
    }
    mono_buffer_[i] = sample;
    square_phase_ = (square_phase_ + 1) % period;
  }
  return write_mono(&mono_buffer_[0], frames);
}

DeckFrameClock::DeckFrameClock(double frames_per_second)
    : start_nanoseconds_(monotonic_nanoseconds()), frame_nanoseconds_(0),
      frame_number_(0) {
  if (frames_per_second > 0.0)
    frame_nanoseconds_ =
        static_cast<int64_t>(std::floor(1000000000.0 / frames_per_second));
}

void DeckFrameClock::wait_for_next_frame() {
  if (frame_nanoseconds_ <= 0)
    return;
  ++frame_number_;
  const int64_t deadline =
      start_nanoseconds_ + static_cast<int64_t>(frame_number_) *
                               frame_nanoseconds_;
  struct timespec target;
  target.tv_sec = deadline / 1000000000LL;
  target.tv_nsec = deadline % 1000000000LL;
  int sleep_result;
  do {
    sleep_result =
        clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &target, NULL);
  } while (sleep_result == EINTR);
  const int64_t now = monotonic_nanoseconds();
  if (now - deadline > frame_nanoseconds_ * 5) {
    start_nanoseconds_ = now;
    frame_number_ = 0;
  }
}
