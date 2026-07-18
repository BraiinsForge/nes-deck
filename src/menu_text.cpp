#include "menu_text.h"

#include <algorithm>
#include <cctype>
#include <climits>
#include <cstdint>

bool is_absolute_path(const std::string &path) {
  return !path.empty() && path[0] == '/' && path.size() < PATH_MAX;
}

std::string trim_ascii_space(const std::string &text) {
  size_t begin = 0;
  while (begin < text.size() &&
         std::isspace(static_cast<unsigned char>(text[begin])))
    ++begin;
  size_t end = text.size();
  while (end > begin &&
         std::isspace(static_cast<unsigned char>(text[end - 1])))
    --end;
  return text.substr(begin, end - begin);
}

bool valid_utf8_text(const std::string &text, size_t max_codepoints,
                     bool allow_empty) {
  if (text.empty())
    return allow_empty;

  size_t count = 0;
  for (size_t i = 0; i < text.size();) {
    const unsigned char first = static_cast<unsigned char>(text[i]);
    uint32_t codepoint = 0;
    size_t length = 0;
    if (first < 0x80) {
      codepoint = first;
      length = 1;
    } else if ((first & 0xe0) == 0xc0) {
      codepoint = first & 0x1f;
      length = 2;
    } else if ((first & 0xf0) == 0xe0) {
      codepoint = first & 0x0f;
      length = 3;
    } else if ((first & 0xf8) == 0xf0) {
      codepoint = first & 0x07;
      length = 4;
    } else {
      return false;
    }
    if (i + length > text.size())
      return false;
    for (size_t j = 1; j < length; ++j) {
      const unsigned char next = static_cast<unsigned char>(text[i + j]);
      if ((next & 0xc0) != 0x80)
        return false;
      codepoint = (codepoint << 6) | (next & 0x3f);
    }
    if ((length == 2 && codepoint < 0x80) ||
        (length == 3 && codepoint < 0x800) ||
        (length == 4 && codepoint < 0x10000) || codepoint > 0x10ffff ||
        (codepoint >= 0xd800 && codepoint <= 0xdfff))
      return false;
    if (codepoint < 0x20 || codepoint == 0x7f)
      return false;
    ++count;
    if (count > max_codepoints)
      return false;
    i += length;
  }
  return true;
}

std::string display_ascii(const std::string &text) {
  std::string result;
  for (size_t i = 0; i < text.size();) {
    const unsigned char first = static_cast<unsigned char>(text[i]);
    if (first < 0x80) {
      result.push_back(static_cast<char>(first));
      ++i;
      continue;
    }
    size_t length = 1;
    if ((first & 0xe0) == 0xc0)
      length = 2;
    else if ((first & 0xf0) == 0xe0)
      length = 3;
    else if ((first & 0xf8) == 0xf0)
      length = 4;
    result.push_back('?');
    i += std::min(length, text.size() - i);
  }
  return result;
}

std::vector<std::string> split_tabs(const std::string &line) {
  std::vector<std::string> fields;
  size_t start = 0;
  while (true) {
    const size_t tab = line.find('\t', start);
    if (tab == std::string::npos) {
      fields.push_back(line.substr(start));
      return fields;
    }
    fields.push_back(line.substr(start, tab - start));
    start = tab + 1;
  }
}
