#ifndef RETRO_DECK_MENU_TEXT_H
#define RETRO_DECK_MENU_TEXT_H

#include <cstddef>
#include <string>
#include <vector>

bool is_absolute_path(const std::string &path);
std::string trim_ascii_space(const std::string &text);
bool valid_utf8_text(const std::string &text, size_t max_codepoints,
                     bool allow_empty);
std::string display_ascii(const std::string &text);
std::vector<std::string> split_tabs(const std::string &line);

#endif
