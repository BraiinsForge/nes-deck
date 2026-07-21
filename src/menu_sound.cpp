#include "menu_sound.h"

#include <algorithm>
#include <cerrno>
#include <csignal>
#include <cstring>
#include <fcntl.h>
#include <iostream>
#include <linux/soundcard.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

namespace {

const int64_t kMenuSoundInputTailMs = 60;

int64_t menu_sound_monotonic_ms() {
  struct timespec now;
  if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
    return 0;
  return static_cast<int64_t>(now.tv_sec) * 1000 + now.tv_nsec / 1000000;
}

std::string menu_sound_errno_message(const std::string &what) {
  return what + ": " + std::strerror(errno);
}

bool menu_sound_write_all(int fd, const char *data, size_t size) {
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

} // namespace

std::vector<ChiptuneNote> menu_sound_notes(MenuSoundCue cue) {
  std::vector<ChiptuneNote> notes;
  if (cue == MenuSoundCueVolume) {
    notes.push_back(ChiptuneNote{660, 60});
    notes.push_back(ChiptuneNote{880, 60});
  } else if (cue == MenuSoundCuePrevious) {
    notes.push_back(ChiptuneNote{523, 35});
  } else if (cue == MenuSoundCueNext) {
    notes.push_back(ChiptuneNote{659, 35});
  } else if (cue == MenuSoundCueConfirm) {
    notes.push_back(ChiptuneNote{659, 25});
    notes.push_back(ChiptuneNote{880, 30});
  } else {
    notes.push_back(ChiptuneNote{659, 25});
    notes.push_back(ChiptuneNote{440, 30});
  }
  return notes;
}

int chiptune_duration_ms(const std::vector<ChiptuneNote> &notes) {
  int duration = 0;
  for (size_t i = 0; i < notes.size(); ++i)
    duration += std::max(0, notes[i].duration_ms);
  return duration;
}

bool render_chiptune(const std::vector<ChiptuneNote> &notes, int rate,
                     unsigned int volume_percent, std::vector<int16_t> *tone,
                     std::string *error) {
  if (!tone || notes.empty()) {
    if (error)
      *error = "chiptune notes and output are required";
    return false;
  }
  if (volume_percent == 0 || volume_percent > 100) {
    if (error)
      *error = "chiptune volume must be between 1 and 100";
    return false;
  }
  if (rate <= 0) {
    if (error)
      *error = "chiptune sample rate must be positive";
    return false;
  }

  const int amplitude =
      std::max(256, static_cast<int>(5000 * volume_percent / 100));
  const size_t ramp_samples =
      std::max<size_t>(1, static_cast<size_t>(rate) / 200);
  tone->clear();
  tone->reserve(static_cast<size_t>(rate) * chiptune_duration_ms(notes) /
                1000);
  for (size_t note_index = 0; note_index < notes.size(); ++note_index) {
    const ChiptuneNote &note = notes[note_index];
    if (note.frequency <= 0 || note.duration_ms <= 0) {
      if (error)
        *error = "chiptune notes must have positive frequency and duration";
      return false;
    }
    const size_t note_samples = std::max<size_t>(
        1, static_cast<size_t>(rate) * note.duration_ms / 1000);
    const size_t period =
        std::max<size_t>(2, static_cast<size_t>(rate / note.frequency));
    const size_t start = tone->size();
    tone->resize(start + note_samples, 0);
    for (size_t i = 0; i < note_samples; ++i) {
      int sample = (i % period) < period / 2 ? amplitude : -amplitude;
      const size_t remaining = note_samples - i;
      const size_t envelope =
          std::min(ramp_samples, std::min(i + 1, remaining));
      sample = static_cast<int>(sample * static_cast<int64_t>(envelope) /
                                static_cast<int64_t>(ramp_samples));
      (*tone)[start + i] = static_cast<int16_t>(sample);
    }
  }
  return true;
}

bool menu_input_quarantined(int64_t quarantine_until, int64_t now) {
  return quarantine_until > now;
}

bool menu_sound_blocks_input(bool sound_active, MenuInputKind input_kind) {
  return sound_active && input_kind == MenuInputController;
}

bool play_chiptune_blocking(const std::vector<ChiptuneNote> &notes,
                            unsigned int volume_percent,
                            std::string *error) {
  const int fd = open("/dev/dsp", O_WRONLY | O_CLOEXEC);
  if (fd < 0) {
    if (error)
      *error = menu_sound_errno_message("cannot open /dev/dsp for menu sound");
    return false;
  }

  int fragment = (4 << 16) | 9;
  int format = AFMT_S16_LE;
  int channels = 1;
  int rate = 44100;
  ioctl(fd, SNDCTL_DSP_SETFRAGMENT, &fragment);
  if (ioctl(fd, SNDCTL_DSP_SETFMT, &format) != 0 ||
      format != AFMT_S16_LE || ioctl(fd, SNDCTL_DSP_CHANNELS, &channels) != 0 ||
      channels != 1 || ioctl(fd, SNDCTL_DSP_SPEED, &rate) != 0 || rate <= 0) {
    const int saved_errno = errno;
    close(fd);
    errno = saved_errno;
    if (error)
      *error = menu_sound_errno_message("cannot configure menu sound");
    return false;
  }

  std::vector<int16_t> tone;
  if (!render_chiptune(notes, rate, volume_percent, &tone, error)) {
    close(fd);
    return false;
  }
  const bool wrote =
      menu_sound_write_all(fd, reinterpret_cast<const char *>(&tone[0]),
                           tone.size() * sizeof(tone[0]));
  const int write_errno = errno;
  if (wrote)
    ioctl(fd, SNDCTL_DSP_SYNC, 0);
  const int close_result = close(fd);
  if (!wrote || close_result != 0) {
    errno = wrote ? errno : write_errno;
    if (error)
      *error = menu_sound_errno_message("cannot play menu sound");
    return false;
  }
  return true;
}

MenuSoundPlayer::MenuSoundPlayer()
    : child_pid_(-1), input_quarantine_until_(0) {}

MenuSoundPlayer::~MenuSoundPlayer() { stop(); }

bool MenuSoundPlayer::play(MenuSoundCue cue, unsigned int volume_percent,
                           std::string *error) {
  if (volume_percent == 0 || volume_percent > 100) {
    if (error)
      *error = "menu sound volume must be between 1 and 100";
    return false;
  }
  reap_finished();
  const int64_t now = menu_sound_monotonic_ms();
  if (child_pid_ > 0)
    return true;
  const std::vector<ChiptuneNote> notes = menu_sound_notes(cue);
  const pid_t child = fork();
  if (child < 0) {
    if (error)
      *error = menu_sound_errno_message("cannot start menu sound worker");
    return false;
  }
  if (child == 0) {
    signal(SIGTERM, SIG_DFL);
    signal(SIGINT, SIG_DFL);
    signal(SIGHUP, SIG_DFL);
    std::string child_error;
    const bool played =
        play_chiptune_blocking(notes, volume_percent, &child_error);
    if (!played)
      std::cerr << "deck-menu: " << child_error << std::endl;
    _exit(played ? 0 : 1);
  }
  child_pid_ = child;
  input_quarantine_until_ =
      now + chiptune_duration_ms(notes) + kMenuSoundInputTailMs;
  return true;
}

void MenuSoundPlayer::reap_finished() {
  if (child_pid_ <= 0)
    return;
  int status = 0;
  const pid_t result = waitpid(child_pid_, &status, WNOHANG);
  if (result == child_pid_) {
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0)
      std::cerr << "deck-menu: menu sound worker failed" << std::endl;
    child_pid_ = -1;
  }
}

bool MenuSoundPlayer::quarantines_input(int64_t now) const {
  return child_pid_ > 0 ||
         menu_input_quarantined(input_quarantine_until_, now);
}

void MenuSoundPlayer::stop() {
  if (child_pid_ > 0) {
    kill(child_pid_, SIGTERM);
    int status = 0;
    while (waitpid(child_pid_, &status, 0) < 0 && errno == EINTR) {
    }
  }
  child_pid_ = -1;
  input_quarantine_until_ = 0;
}

void MenuSoundPlayer::finish() {
  if (child_pid_ > 0) {
    int status = 0;
    pid_t result = -1;
    do {
      result = waitpid(child_pid_, &status, 0);
    } while (result < 0 && errno == EINTR);
    if (result == child_pid_ &&
        (!WIFEXITED(status) || WEXITSTATUS(status) != 0)) {
      std::cerr << "deck-menu: menu sound worker failed" << std::endl;
    } else if (result < 0 && errno != ECHILD) {
      std::cerr << "deck-menu: cannot finish menu sound worker: "
                << std::strerror(errno) << std::endl;
    }
  }
  child_pid_ = -1;
  input_quarantine_until_ = 0;
}
