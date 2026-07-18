#ifndef RETRO_DECK_MENU_SOUND_H
#define RETRO_DECK_MENU_SOUND_H

#include <cstdint>
#include <string>
#include <sys/types.h>
#include <vector>

struct ChiptuneNote {
  int frequency;
  int duration_ms;
};

enum MenuSoundCue {
  MenuSoundCueVolume,
  MenuSoundCuePrevious,
  MenuSoundCueNext,
  MenuSoundCueConfirm,
  MenuSoundCueBack
};

enum MenuInputKind {
  MenuInputTouch,
  MenuInputController,
  MenuInputKeyboard
};

std::vector<ChiptuneNote> menu_sound_notes(MenuSoundCue cue);
int chiptune_duration_ms(const std::vector<ChiptuneNote> &notes);
bool render_chiptune(const std::vector<ChiptuneNote> &notes, int rate,
                     unsigned int volume_percent, std::vector<int16_t> *tone,
                     std::string *error);
bool menu_input_quarantined(int64_t quarantine_until, int64_t now);
bool menu_sound_blocks_input(bool sound_active, MenuInputKind input_kind);
bool play_chiptune_blocking(const std::vector<ChiptuneNote> &notes,
                            unsigned int volume_percent,
                            std::string *error);

class MenuSoundPlayer {
public:
  MenuSoundPlayer();
  ~MenuSoundPlayer();

  bool play(MenuSoundCue cue, unsigned int volume_percent,
            std::string *error);
  void reap_finished();
  bool quarantines_input(int64_t now) const;
  void stop();
  void finish();

private:
  MenuSoundPlayer(const MenuSoundPlayer &);
  MenuSoundPlayer &operator=(const MenuSoundPlayer &);

  pid_t child_pid_;
  int64_t input_quarantine_until_;
};

#endif
