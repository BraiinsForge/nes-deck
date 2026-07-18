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

const int kCreditsCrawlTextScale = 4;

struct CreditsCrawlLine {
  std::string text;
  int source_y;
  int source_width;
  int source_height;
  std::vector<uint8_t> pixels;
};

struct CreditsCrawl {
  std::vector<CreditsCrawlLine> lines;
  std::vector<std::string> static_lines;
  int content_height;
};

struct CreditsLayout {
  Rect close_button;
};

enum CreditsTarget { CreditsTargetNone = -1, CreditsTargetClose = 0 };

bool load_project_credits(const std::string &path,
                          std::vector<ProjectCredit> *credits,
                          std::string *error);

CreditsCrawl make_project_credits_crawl(
    const std::vector<ProjectCredit> &credits);

void render_project_credits(const CreditsCrawl &crawl,
                            bool reduced_motion, int64_t elapsed_ms,
                            uint16_t background, uint16_t accent,
                            uint16_t text, uint16_t muted, Canvas *canvas,
                            CreditsLayout *layout);

int credits_target_at(const CreditsLayout &layout, int x, int y);

#endif
