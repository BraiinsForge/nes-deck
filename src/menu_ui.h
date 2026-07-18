#ifndef RETRO_DECK_MENU_UI_H
#define RETRO_DECK_MENU_UI_H

#include <cstdint>
#include <string>
#include <vector>

const int kLogicalWidth = 1280;
const int kLogicalHeight = 480;
const int kBitmapGlyphWidth = 5;
const int kBitmapGlyphHeight = 7;
const int kBitmapGlyphAdvance = 6;

struct Rect {
  int x;
  int y;
  int width;
  int height;

  bool contains(int px, int py) const {
    return px >= x && py >= y && px < x + width && py < y + height;
  }
};

typedef std::vector<uint16_t> Canvas;

std::string display_ascii(const std::string &text);

void fill_rect(Canvas *canvas, const Rect &rect, uint16_t color);
void stroke_rect(Canvas *canvas, const Rect &rect, int thickness,
                 uint16_t color);
void fill_pixel_cut_rect(Canvas *canvas, const Rect &rect, int cut,
                         uint16_t color);

int text_width(const std::string &text, int scale);
bool bitmap_glyph_pixel(char character, int column, int row);
void draw_text(Canvas *canvas, int x, int y, const std::string &text, int scale,
               uint16_t color);
void draw_centered_text(Canvas *canvas, const Rect &bounds,
                        const std::string &text, int scale, uint16_t color);
int fit_text_scale(const std::string &text, int maximum_width, int preferred,
                   int minimum);
std::string fit_text_width(const std::string &text, int maximum_width,
                           int scale);

#endif
