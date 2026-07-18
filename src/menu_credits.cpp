#include "menu_credits.h"

#include <algorithm>
#include <cerrno>
#include <cmath>
#include <cstring>
#include <fstream>
#include <set>
#include <sstream>
#include <sys/stat.h>

namespace {

const off_t kMaximumCreditsBytes = 32768;
const size_t kMaximumCredits = 64;
const int kCrawlMaximumLineWidth = 1040;
const int kCrawlLineAdvance = 44;
const int kCrawlSectionGap = 28;
const int kCrawlHorizonY = 56;
const int kCrawlClipTop = 72;
const int kCrawlFadeInvisibleY = 104;
const int kCrawlFadeOpaqueY = 210;
const int kCrawlBottomY = kLogicalHeight;
const double kCrawlCameraDistance = 420.0;
const double kCrawlMaximumDepth = 4000.0;
const double kCrawlSourceUnitsPerMillisecond = 0.05;

std::string system_error(const std::string &what) {
  return what + ": " + std::strerror(errno);
}

std::vector<std::string> split_tabs(const std::string &line) {
  std::vector<std::string> fields;
  size_t begin = 0;
  while (true) {
    const size_t tab = line.find('\t', begin);
    if (tab == std::string::npos) {
      fields.push_back(line.substr(begin));
      return fields;
    }
    fields.push_back(line.substr(begin, tab - begin));
    begin = tab + 1;
  }
}

bool valid_field(const std::string &field, size_t maximum) {
  if (field.empty() || field.size() > maximum)
    return false;
  for (size_t index = 0; index < field.size(); ++index) {
    const unsigned char ch = static_cast<unsigned char>(field[index]);
    if (ch < 0x20 || ch > 0x7e || ch == '\t')
      return false;
  }
  return true;
}

std::vector<std::string> wrap_crawl_text(const std::string &input) {
  std::vector<std::string> wrapped;
  std::string remaining = display_ascii(input);
  const size_t maximum_characters = static_cast<size_t>(
      (kCrawlMaximumLineWidth + kCreditsCrawlTextScale) /
      (kBitmapGlyphAdvance * kCreditsCrawlTextScale));
  while (!remaining.empty()) {
    while (!remaining.empty() && remaining[0] == ' ')
      remaining.erase(0, 1);
    if (remaining.empty())
      break;
    if (text_width(remaining, kCreditsCrawlTextScale) <=
        kCrawlMaximumLineWidth) {
      wrapped.push_back(remaining);
      break;
    }
    size_t split = remaining.rfind(' ', maximum_characters);
    if (split == std::string::npos || split == 0)
      split = maximum_characters;
    wrapped.push_back(remaining.substr(0, split));
    remaining.erase(0, split);
  }
  return wrapped;
}

void append_crawl_text(CreditsCrawl *crawl, int *cursor,
                       const std::string &text) {
  const std::vector<std::string> wrapped = wrap_crawl_text(text);
  for (size_t index = 0; index < wrapped.size(); ++index) {
    CreditsCrawlLine line;
    line.text = wrapped[index];
    line.source_y = *cursor;
    line.source_width = text_width(line.text, kCreditsCrawlTextScale);
    line.source_height = kBitmapGlyphHeight * kCreditsCrawlTextScale;
    line.pixels.assign(
        static_cast<size_t>(line.source_width * line.source_height), 0);
    for (size_t character = 0; character < line.text.size(); ++character) {
      for (int row = 0; row < kBitmapGlyphHeight; ++row) {
        for (int column = 0; column < kBitmapGlyphWidth; ++column) {
          if (!bitmap_glyph_pixel(line.text[character], column, row))
            continue;
          const int left =
              (static_cast<int>(character) * kBitmapGlyphAdvance + column) *
              kCreditsCrawlTextScale;
          const int top = row * kCreditsCrawlTextScale;
          for (int pixel_y = top;
               pixel_y < top + kCreditsCrawlTextScale; ++pixel_y) {
            std::fill(line.pixels.begin() + pixel_y * line.source_width + left,
                      line.pixels.begin() + pixel_y * line.source_width +
                          left + kCreditsCrawlTextScale,
                      1);
          }
        }
      }
    }
    crawl->lines.push_back(line);
    *cursor += kCrawlLineAdvance;
  }
}

void draw_starfield(Canvas *canvas, uint16_t color) {
  for (unsigned int index = 0; index < 96; ++index) {
    if (index % 7 == 0)
      continue;
    const int x = static_cast<int>((index * 193U + 47U) % kLogicalWidth);
    const int y = static_cast<int>((index * 83U + 29U) % kLogicalHeight);
    const int size = index % 11 == 0 ? 2 : 1;
    fill_rect(canvas, Rect{x, y, size, size}, color);
  }
}

double crawl_scale(double depth) {
  return kCrawlCameraDistance / (kCrawlCameraDistance + depth);
}

double crawl_screen_y(double depth) {
  return kCrawlHorizonY +
         (kCrawlBottomY - kCrawlHorizonY) * crawl_scale(depth);
}

int crawl_alpha(int screen_y) {
  if (screen_y <= kCrawlFadeInvisibleY)
    return 0;
  if (screen_y >= kCrawlFadeOpaqueY)
    return 256;
  return (screen_y - kCrawlFadeInvisibleY) * 256 /
         (kCrawlFadeOpaqueY - kCrawlFadeInvisibleY);
}

uint16_t blend_rgb565(uint16_t foreground, uint16_t background, int alpha) {
  const int inverse = 256 - alpha;
  const int red = (((foreground >> 11) & 0x1f) * alpha +
                   ((background >> 11) & 0x1f) * inverse + 128) >>
                  8;
  const int green = (((foreground >> 5) & 0x3f) * alpha +
                     ((background >> 5) & 0x3f) * inverse + 128) >>
                    8;
  const int blue = ((foreground & 0x1f) * alpha +
                    (background & 0x1f) * inverse + 128) >>
                   8;
  return static_cast<uint16_t>((red << 11) | (green << 5) | blue);
}

void draw_crawl_line(Canvas *canvas, const CreditsCrawlLine &line,
                     double scroll, uint16_t color) {
  if (line.source_width <= 0 || line.source_height <= 0 ||
      line.pixels.size() !=
          static_cast<size_t>(line.source_width * line.source_height))
    return;
  const double source_top = std::max(
      static_cast<double>(line.source_y), scroll - kCrawlMaximumDepth);
  const double source_bottom = std::min(
      static_cast<double>(line.source_y + line.source_height), scroll);
  if (source_top >= source_bottom)
    return;

  const double top_y = crawl_screen_y(scroll - source_top);
  const double bottom_y = crawl_screen_y(scroll - source_bottom);
  const int first_y = std::max(kCrawlClipTop,
                               static_cast<int>(std::floor(top_y)));
  const int last_y = std::min(kCrawlBottomY - 1,
                              static_cast<int>(std::ceil(bottom_y)) - 1);
  const double projection_height = kCrawlBottomY - kCrawlHorizonY;
  for (int y = first_y; y <= last_y; ++y) {
    const double scale = (y + 0.5 - kCrawlHorizonY) / projection_height;
    if (scale <= 0.0)
      continue;
    const double depth = kCrawlCameraDistance * (1.0 / scale - 1.0);
    const int source_row = static_cast<int>(
        std::floor(scroll - depth - line.source_y));
    if (source_row < 0 || source_row >= line.source_height)
      continue;
    const double left = kLogicalWidth * 0.5 -
                        line.source_width * 0.5 * scale;
    const double right = kLogicalWidth * 0.5 +
                         line.source_width * 0.5 * scale;
    const int first_x = std::max(
        0, static_cast<int>(std::ceil(left - 0.5)));
    const int last_x = std::min(
        kLogicalWidth - 1, static_cast<int>(std::floor(right - 0.5)));
    const int alpha = crawl_alpha(y);
    if (alpha <= 0)
      continue;
    for (int x = first_x; x <= last_x; ++x) {
      const int source_column = static_cast<int>(std::floor(
          (x + 0.5 - kLogicalWidth * 0.5) / scale +
          line.source_width * 0.5));
      if (source_column < 0 || source_column >= line.source_width ||
          line.pixels[static_cast<size_t>(source_row) * line.source_width +
                      source_column] == 0)
        continue;
      uint16_t &pixel =
          (*canvas)[static_cast<size_t>(y) * kLogicalWidth + x];
      pixel = alpha == 256 ? color : blend_rgb565(color, pixel, alpha);
    }
  }
}

void draw_static_credits(const CreditsCrawl &crawl, uint16_t accent,
                         uint16_t text, uint16_t muted, Canvas *canvas) {
  draw_text(canvas, 20, 20, "FOSS CREDITS", 2, accent);
  draw_text(canvas, 20, 48, "PROJECT / LICENSE", 1, muted);
  const size_t rows_per_column = 16;
  const size_t columns = std::max<size_t>(
      1, (crawl.static_lines.size() + rows_per_column - 1) /
             rows_per_column);
  const int left_margin = 24;
  const int column_width =
      (kLogicalWidth - left_margin * 2) / static_cast<int>(columns);
  for (size_t index = 0; index < crawl.static_lines.size(); ++index) {
    const size_t column = index / rows_per_column;
    const size_t row = index % rows_per_column;
    const std::string shown = fit_text_width(
        crawl.static_lines[index], column_width - 20, 1);
    draw_text(canvas,
              left_margin + static_cast<int>(column) * column_width,
              78 + static_cast<int>(row) * 22, shown, 1, text);
  }
  draw_text(canvas, 20, 458, "/mnt/data/nes-deck/licenses", 1, muted);
}

void draw_close(Canvas *canvas, const Rect &bounds, uint16_t color) {
  const int center_x = bounds.x + bounds.width / 2;
  const int center_y = bounds.y + bounds.height / 2;
  for (int offset = -12; offset <= 12; offset += 4) {
    fill_rect(canvas, Rect{center_x + offset, center_y + offset, 4, 4}, color);
    fill_rect(canvas, Rect{center_x + offset, center_y - offset, 4, 4}, color);
  }
}

} // namespace

bool load_project_credits(const std::string &path,
                          std::vector<ProjectCredit> *credits,
                          std::string *error) {
  if (!credits || path.empty() || path[0] != '/') {
    if (error)
      *error = "credits path must be absolute";
    return false;
  }

  struct stat info;
  if (lstat(path.c_str(), &info) != 0) {
    if (error)
      *error = system_error("cannot inspect credits " + path);
    return false;
  }
  if (!S_ISREG(info.st_mode) || info.st_size <= 0 ||
      info.st_size > kMaximumCreditsBytes) {
    if (error)
      *error = "credits must be a non-empty regular file no larger than 32 KiB";
    return false;
  }

  std::ifstream input(path.c_str());
  if (!input) {
    if (error)
      *error = "cannot open credits " + path;
    return false;
  }

  std::vector<ProjectCredit> loaded;
  std::set<std::string> projects;
  std::string line;
  size_t line_number = 0;
  while (std::getline(input, line)) {
    ++line_number;
    if (!line.empty() && line[line.size() - 1] == '\r')
      line.erase(line.size() - 1);
    if (line.empty() || line[0] == '#')
      continue;
    const std::vector<std::string> fields = split_tabs(line);
    if (fields.size() != 3 || !valid_field(fields[0], 48) ||
        !valid_field(fields[1], 64) || !valid_field(fields[2], 64)) {
      if (error) {
        std::ostringstream message;
        message << "invalid credits row " << line_number;
        *error = message.str();
      }
      return false;
    }
    if (!projects.insert(fields[0]).second) {
      if (error)
        *error = "duplicate credits project " + fields[0];
      return false;
    }
    loaded.push_back(ProjectCredit{fields[0], fields[1], fields[2]});
    if (loaded.size() > kMaximumCredits) {
      if (error)
        *error = "credits contain more than 64 projects";
      return false;
    }
  }
  if (input.bad() || loaded.empty()) {
    if (error)
      *error = input.bad() ? "cannot read credits " + path
                           : "credits contain no projects";
    return false;
  }
  credits->swap(loaded);
  return true;
}

CreditsCrawl make_project_credits_crawl(
    const std::vector<ProjectCredit> &credits) {
  CreditsCrawl crawl;
  crawl.content_height = 0;
  if (credits.empty())
    return crawl;

  int cursor = 0;
  append_crawl_text(&crawl, &cursor, "RETRO DECK");
  append_crawl_text(&crawl, &cursor, "BUILT ON FREE SOFTWARE");
  cursor += kCrawlSectionGap;
  for (size_t index = 0; index < credits.size(); ++index) {
    crawl.static_lines.push_back(display_ascii(
        credits[index].project + " / " + credits[index].license));
    append_crawl_text(&crawl, &cursor, credits[index].project);
    append_crawl_text(&crawl, &cursor, credits[index].role);
    append_crawl_text(&crawl, &cursor, credits[index].license);
    cursor += kCrawlSectionGap;
  }
  append_crawl_text(&crawl, &cursor, "LICENSE TEXT ARCHIVE");
  append_crawl_text(&crawl, &cursor, "/mnt/data/nes-deck/licenses");
  cursor += kCrawlSectionGap;
  append_crawl_text(&crawl, &cursor, "THANK YOU");
  crawl.content_height = cursor;
  return crawl;
}

void render_project_credits(const CreditsCrawl &crawl,
                            bool reduced_motion, int64_t elapsed_ms,
                            uint16_t background, uint16_t accent,
                            uint16_t text, uint16_t muted, Canvas *canvas,
                            CreditsLayout *layout) {
  if (!canvas || !layout)
    return;
  canvas->assign(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                 background);
  layout->close_button = Rect{1212, 12, 56, 56};
  if (!reduced_motion)
    draw_starfield(canvas, muted);

  if (crawl.lines.empty() || crawl.content_height <= 0) {
    draw_centered_text(canvas, Rect{80, 180, 1120, 120},
                       "CREDITS UNAVAILABLE", 3, text);
  } else if (reduced_motion) {
    draw_static_credits(crawl, accent, text, muted, canvas);
  } else {
    const double cycle = crawl.content_height + kCrawlMaximumDepth;
    const double elapsed =
        static_cast<double>(std::max<int64_t>(0, elapsed_ms));
    const double scroll = std::fmod(
        elapsed * kCrawlSourceUnitsPerMillisecond, cycle);
    for (size_t index = 0; index < crawl.lines.size(); ++index)
      draw_crawl_line(canvas, crawl.lines[index], scroll, accent);
  }

  draw_close(canvas, layout->close_button, muted);
}

int credits_target_at(const CreditsLayout &layout, int x, int y) {
  return layout.close_button.contains(x, y) ? CreditsTargetClose
                                            : CreditsTargetNone;
}
