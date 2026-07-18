#include "menu_credits.h"

#include <algorithm>
#include <cerrno>
#include <cstring>
#include <fstream>
#include <set>
#include <sstream>
#include <sys/stat.h>

namespace {

const off_t kMaximumCreditsBytes = 32768;
const size_t kMaximumCredits = 64;

enum CrawlLineKind { CrawlHeading, CrawlProject, CrawlRole, CrawlLicense };

struct CrawlLine {
  std::string text;
  CrawlLineKind kind;
};

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

std::vector<CrawlLine>
build_crawl_lines(const std::vector<ProjectCredit> &credits) {
  std::vector<CrawlLine> lines;
  lines.push_back(CrawlLine{"RETRO DECK", CrawlHeading});
  lines.push_back(CrawlLine{"BUILT ON FREE SOFTWARE", CrawlHeading});
  lines.push_back(CrawlLine{"", CrawlHeading});
  for (size_t index = 0; index < credits.size(); ++index) {
    lines.push_back(CrawlLine{credits[index].project, CrawlProject});
    lines.push_back(CrawlLine{credits[index].role, CrawlRole});
    lines.push_back(CrawlLine{credits[index].license, CrawlLicense});
    lines.push_back(CrawlLine{"", CrawlRole});
  }
  lines.push_back(CrawlLine{"LICENSE TEXT ARCHIVE", CrawlHeading});
  lines.push_back(
      CrawlLine{"/mnt/data/nes-deck/licenses", CrawlLicense});
  lines.push_back(CrawlLine{"", CrawlRole});
  lines.push_back(CrawlLine{"THANK YOU", CrawlHeading});
  return lines;
}

void draw_starfield(Canvas *canvas, int64_t elapsed_ms, uint16_t color) {
  const unsigned int phase = static_cast<unsigned int>(elapsed_ms / 240);
  for (unsigned int index = 0; index < 96; ++index) {
    if ((index + phase) % 7 == 0)
      continue;
    const int x = static_cast<int>((index * 193U + 47U) % kLogicalWidth);
    const int y = static_cast<int>((index * 83U + 29U) % kLogicalHeight);
    const int size = (index + phase) % 11 == 0 ? 2 : 1;
    fill_rect(canvas, Rect{x, y, size, size}, color);
  }
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

void render_project_credits(const std::vector<ProjectCredit> &credits,
                            int64_t elapsed_ms, uint16_t background,
                            uint16_t accent, uint16_t text, uint16_t muted,
                            Canvas *canvas, CreditsLayout *layout) {
  if (!canvas || !layout)
    return;
  canvas->assign(static_cast<size_t>(kLogicalWidth * kLogicalHeight),
                 background);
  layout->close_button = Rect{1212, 12, 56, 56};
  draw_starfield(canvas, std::max<int64_t>(0, elapsed_ms), muted);
  draw_text(canvas, 20, 20, "FOSS CREDITS", 2, accent);
  draw_close(canvas, layout->close_button, muted);
  draw_text(canvas, 20, 458, "B TO CLOSE", 1, muted);

  if (credits.empty()) {
    draw_centered_text(canvas, Rect{80, 180, 1120, 120},
                       "CREDITS UNAVAILABLE", 3, text);
    return;
  }

  const std::vector<CrawlLine> lines = build_crawl_lines(credits);
  const int line_spacing = 32;
  const int content_height = static_cast<int>(lines.size()) * line_spacing;
  const int cycle_distance = content_height + 700;
  const int travel = static_cast<int>(std::max<int64_t>(0, elapsed_ms) / 30 %
                                      cycle_distance);
  const int first_y = 372 - travel;
  for (size_t index = 0; index < lines.size(); ++index) {
    if (lines[index].text.empty())
      continue;
    const int y = first_y + static_cast<int>(index) * line_spacing;
    if (y < 76 || y >= kLogicalHeight - 16)
      continue;
    int scale = y >= 344 ? 3 : (y >= 208 ? 2 : 1);
    scale = fit_text_scale(lines[index].text, kLogicalWidth - 160, scale, 1);
    const uint16_t color =
        lines[index].kind == CrawlProject ||
                lines[index].kind == CrawlHeading
            ? accent
            : (lines[index].kind == CrawlLicense || y < 128 ? muted : text);
    draw_centered_text(canvas, Rect{80, y, kLogicalWidth - 160, 7 * scale},
                       lines[index].text, scale, color);
  }
}

int credits_target_at(const CreditsLayout &layout, int x, int y) {
  return layout.close_button.contains(x, y) ? CreditsTargetClose
                                            : CreditsTargetNone;
}
