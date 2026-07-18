#ifndef RETRO_DECK_MENU_CREDITS_H
#define RETRO_DECK_MENU_CREDITS_H

#include "menu_ui.h"

#include <cstdint>
#include <string>
#include <vector>

struct ProjectCredit {
  std::string project;
  std::string role;
  std::string license;
};

struct CreditsLayout {
  Rect close_button;
};

enum CreditsTarget { CreditsTargetNone = -1, CreditsTargetClose = 0 };

bool load_project_credits(const std::string &path,
                          std::vector<ProjectCredit> *credits,
                          std::string *error);

void render_project_credits(const std::vector<ProjectCredit> &credits,
                            int64_t elapsed_ms, uint16_t background,
                            uint16_t accent, uint16_t text, uint16_t muted,
                            Canvas *canvas, CreditsLayout *layout);

int credits_target_at(const CreditsLayout &layout, int x, int y);

#endif
