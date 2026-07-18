#include "menu_ui.h"

#include <algorithm>

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

void fill_rect(Canvas *canvas, const Rect &rect, uint16_t color) {
  if (!canvas || canvas->size() !=
                     static_cast<size_t>(kLogicalWidth * kLogicalHeight))
    return;
  const int left = std::max(0, rect.x);
  const int top = std::max(0, rect.y);
  const int right = std::min(kLogicalWidth, rect.x + rect.width);
  const int bottom = std::min(kLogicalHeight, rect.y + rect.height);
  for (int y = top; y < bottom; ++y) {
    std::fill(canvas->begin() + y * kLogicalWidth + left,
              canvas->begin() + y * kLogicalWidth + right, color);
  }
}

void stroke_rect(Canvas *canvas, const Rect &rect, int thickness,
                 uint16_t color) {
  fill_rect(canvas, Rect{rect.x, rect.y, rect.width, thickness}, color);
  fill_rect(canvas,
            Rect{rect.x, rect.y + rect.height - thickness, rect.width,
                 thickness},
            color);
  fill_rect(canvas, Rect{rect.x, rect.y, thickness, rect.height}, color);
  fill_rect(canvas,
            Rect{rect.x + rect.width - thickness, rect.y, thickness,
                 rect.height},
            color);
}

void fill_pixel_cut_rect(Canvas *canvas, const Rect &rect, int cut,
                         uint16_t color) {
  if (rect.width <= cut * 2 || rect.height <= cut * 2)
    return;
  fill_rect(canvas,
            Rect{rect.x + cut, rect.y, rect.width - cut * 2, rect.height},
            color);
  fill_rect(canvas,
            Rect{rect.x, rect.y + cut, rect.width, rect.height - cut * 2},
            color);
}

namespace {

const uint8_t *glyph_rows(char input) {
  static const uint8_t space[7] = {0, 0, 0, 0, 0, 0, 0};
  static const uint8_t unknown[7] = {14, 17, 1, 2, 4, 0, 4};
  static const uint8_t digits[10][7] = {
      {14, 17, 19, 21, 25, 17, 14}, {4, 12, 4, 4, 4, 4, 14},
      {14, 17, 1, 2, 4, 8, 31},    {30, 1, 1, 14, 1, 1, 30},
      {2, 6, 10, 18, 31, 2, 2},    {31, 16, 16, 30, 1, 1, 30},
      {14, 16, 16, 30, 17, 17, 14}, {31, 1, 2, 4, 8, 8, 8},
      {14, 17, 17, 14, 17, 17, 14}, {14, 17, 17, 15, 1, 1, 14}};
  static const uint8_t letters[26][7] = {
      {14, 17, 17, 31, 17, 17, 17}, {30, 17, 17, 30, 17, 17, 30},
      {14, 17, 16, 16, 16, 17, 14}, {30, 17, 17, 17, 17, 17, 30},
      {31, 16, 16, 30, 16, 16, 31}, {31, 16, 16, 30, 16, 16, 16},
      {14, 17, 16, 23, 17, 17, 15}, {17, 17, 17, 31, 17, 17, 17},
      {14, 4, 4, 4, 4, 4, 14},      {7, 2, 2, 2, 18, 18, 12},
      {17, 18, 20, 24, 20, 18, 17}, {16, 16, 16, 16, 16, 16, 31},
      {17, 27, 21, 21, 17, 17, 17}, {17, 25, 21, 19, 17, 17, 17},
      {14, 17, 17, 17, 17, 17, 14}, {30, 17, 17, 30, 16, 16, 16},
      {14, 17, 17, 17, 21, 18, 13}, {30, 17, 17, 30, 20, 18, 17},
      {15, 16, 16, 14, 1, 1, 30},   {31, 4, 4, 4, 4, 4, 4},
      {17, 17, 17, 17, 17, 17, 14}, {17, 17, 17, 17, 17, 10, 4},
      {17, 17, 17, 17, 21, 21, 10}, {17, 17, 10, 4, 10, 17, 17},
      {17, 17, 10, 4, 4, 4, 4},     {31, 1, 2, 4, 8, 16, 31}};
  static const uint8_t lowercase[26][7] = {
      {0, 0, 14, 1, 15, 17, 15},    {16, 16, 30, 17, 17, 17, 30},
      {0, 0, 14, 16, 16, 17, 14},   {1, 1, 15, 17, 17, 17, 15},
      {0, 0, 14, 17, 31, 16, 14},   {6, 9, 8, 28, 8, 8, 8},
      {0, 0, 15, 17, 15, 1, 14},    {16, 16, 30, 17, 17, 17, 17},
      {4, 0, 12, 4, 4, 4, 14},      {2, 0, 6, 2, 2, 18, 12},
      {16, 16, 18, 20, 24, 20, 18}, {12, 4, 4, 4, 4, 4, 14},
      {0, 0, 26, 21, 21, 17, 17},   {0, 0, 30, 17, 17, 17, 17},
      {0, 0, 14, 17, 17, 17, 14},   {0, 0, 30, 17, 30, 16, 16},
      {0, 0, 15, 17, 15, 1, 1},     {0, 0, 22, 25, 16, 16, 16},
      {0, 0, 15, 16, 14, 1, 30},    {8, 8, 28, 8, 8, 9, 6},
      {0, 0, 17, 17, 17, 19, 13},   {0, 0, 17, 17, 17, 10, 4},
      {0, 0, 17, 17, 21, 21, 10},   {0, 0, 17, 10, 4, 10, 17},
      {0, 0, 17, 17, 15, 1, 14},    {0, 0, 31, 2, 4, 8, 31}};
  static const uint8_t period[7] = {0, 0, 0, 0, 0, 6, 6};
  static const uint8_t comma[7] = {0, 0, 0, 0, 6, 6, 4};
  static const uint8_t colon[7] = {0, 6, 6, 0, 6, 6, 0};
  static const uint8_t dash[7] = {0, 0, 0, 31, 0, 0, 0};
  static const uint8_t slash[7] = {1, 2, 2, 4, 8, 8, 16};
  static const uint8_t plus[7] = {0, 4, 4, 31, 4, 4, 0};
  static const uint8_t bang[7] = {4, 4, 4, 4, 4, 0, 4};
  static const uint8_t question[7] = {14, 17, 1, 2, 4, 0, 4};
  static const uint8_t apostrophe[7] = {4, 4, 8, 0, 0, 0, 0};
  static const uint8_t left_paren[7] = {2, 4, 8, 8, 8, 4, 2};
  static const uint8_t right_paren[7] = {8, 4, 2, 2, 2, 4, 8};
  static const uint8_t ampersand[7] = {12, 18, 20, 8, 21, 18, 13};
  static const uint8_t hash[7] = {10, 10, 31, 10, 31, 10, 10};
  static const uint8_t underscore[7] = {0, 0, 0, 0, 0, 0, 31};
  static const uint8_t semicolon[7] = {0, 6, 6, 0, 6, 6, 4};
  static const uint8_t equal[7] = {0, 31, 0, 31, 0, 0, 0};
  static const uint8_t quote[7] = {10, 10, 20, 0, 0, 0, 0};
  static const uint8_t star[7] = {0, 21, 14, 31, 14, 21, 0};
  static const uint8_t percent[7] = {25, 25, 2, 4, 8, 19, 19};
  static const uint8_t caret[7] = {4, 10, 17, 0, 0, 0, 0};
  static const uint8_t pipe[7] = {4, 4, 4, 4, 4, 4, 4};
  static const uint8_t backslash[7] = {16, 8, 8, 4, 2, 2, 1};
  static const uint8_t less[7] = {2, 4, 8, 16, 8, 4, 2};
  static const uint8_t greater[7] = {8, 4, 2, 1, 2, 4, 8};
  static const uint8_t left_bracket[7] = {14, 8, 8, 8, 8, 8, 14};
  static const uint8_t right_bracket[7] = {14, 2, 2, 2, 2, 2, 14};
  static const uint8_t left_brace[7] = {6, 4, 4, 24, 4, 4, 6};
  static const uint8_t right_brace[7] = {12, 4, 4, 3, 4, 4, 12};
  static const uint8_t at[7] = {14, 17, 23, 21, 23, 16, 14};
  static const uint8_t dollar[7] = {4, 15, 20, 14, 5, 30, 4};
  static const uint8_t backtick[7] = {8, 4, 0, 0, 0, 0, 0};
  static const uint8_t tilde[7] = {0, 0, 9, 22, 0, 0, 0};

  unsigned char ch = static_cast<unsigned char>(input);
  if (ch >= 'a' && ch <= 'z')
    return lowercase[ch - 'a'];
  if (ch >= 'A' && ch <= 'Z')
    return letters[ch - 'A'];
  if (ch >= '0' && ch <= '9')
    return digits[ch - '0'];
  switch (ch) {
  case ' ': return space;
  case '.': return period;
  case ',': return comma;
  case ':': return colon;
  case '-': return dash;
  case '/': return slash;
  case '+': return plus;
  case '!': return bang;
  case '?': return question;
  case '\'': return apostrophe;
  case '(': return left_paren;
  case ')': return right_paren;
  case '&': return ampersand;
  case '#': return hash;
  case '_': return underscore;
  case ';': return semicolon;
  case '=': return equal;
  case '"': return quote;
  case '*': return star;
  case '%': return percent;
  case '^': return caret;
  case '|': return pipe;
  case '\\': return backslash;
  case '<': return less;
  case '>': return greater;
  case '[': return left_bracket;
  case ']': return right_bracket;
  case '{': return left_brace;
  case '}': return right_brace;
  case '@': return at;
  case '$': return dollar;
  case '`': return backtick;
  case '~': return tilde;
  default: return unknown;
  }
}

void draw_character(Canvas *canvas, int x, int y, char ch, int scale,
                    uint16_t color) {
  const uint8_t *rows = glyph_rows(ch);
  for (int row = 0; row < kBitmapGlyphHeight; ++row) {
    for (int column = 0; column < kBitmapGlyphWidth; ++column) {
      if (rows[row] & (1u << (kBitmapGlyphWidth - 1 - column)))
        fill_rect(canvas,
                  Rect{x + column * scale, y + row * scale, scale, scale},
                  color);
    }
  }
}

} // namespace

int text_width(const std::string &text, int scale) {
  if (text.empty())
    return 0;
  return static_cast<int>(text.size()) * kBitmapGlyphAdvance * scale - scale;
}

bool bitmap_glyph_pixel(char character, int column, int row) {
  if (column < 0 || column >= kBitmapGlyphWidth || row < 0 ||
      row >= kBitmapGlyphHeight)
    return false;
  return (glyph_rows(character)[row] &
          (1u << (kBitmapGlyphWidth - 1 - column))) != 0;
}

void draw_text(Canvas *canvas, int x, int y, const std::string &utf8_text,
               int scale, uint16_t color) {
  const std::string text = display_ascii(utf8_text);
  for (size_t i = 0; i < text.size(); ++i)
    draw_character(canvas,
                   x + static_cast<int>(i) * kBitmapGlyphAdvance * scale, y,
                   text[i], scale, color);
}

void draw_centered_text(Canvas *canvas, const Rect &bounds,
                        const std::string &text, int scale, uint16_t color) {
  const std::string shown = display_ascii(text);
  const int width = text_width(shown, scale);
  const int height = kBitmapGlyphHeight * scale;
  draw_text(canvas, bounds.x + std::max(0, (bounds.width - width) / 2),
            bounds.y + std::max(0, (bounds.height - height) / 2), shown,
            scale, color);
}

int fit_text_scale(const std::string &text, int maximum_width, int preferred,
                   int minimum) {
  const std::string shown = display_ascii(text);
  for (int scale = preferred; scale >= minimum; --scale) {
    if (text_width(shown, scale) <= maximum_width)
      return scale;
  }
  return minimum;
}

std::string fit_text_width(const std::string &text, int maximum_width,
                           int scale) {
  std::string shown = display_ascii(text);
  if (text_width(shown, scale) <= maximum_width)
    return shown;
  const int character_width = kBitmapGlyphAdvance * scale;
  const size_t capacity = maximum_width > 0
                              ? static_cast<size_t>((maximum_width + scale) /
                                                    character_width)
                              : 0;
  if (capacity == 0)
    return std::string();
  if (capacity <= 3)
    return shown.substr(0, capacity);
  shown.resize(capacity - 3);
  shown += "...";
  return shown;
}
