#include <algorithm>
#include <cassert>
#include <iostream>
#include <set>

#include "../src/menu_credits.h"

namespace {

uint16_t rgb565(uint32_t color) {
  const uint32_t red = (color >> 16) & 0xff;
  const uint32_t green = (color >> 8) & 0xff;
  const uint32_t blue = color & 0xff;
  return static_cast<uint16_t>(((red & 0xf8) << 8) |
                               ((green & 0xfc) << 3) | (blue >> 3));
}

uint64_t canvas_hash(const Canvas &canvas) {
  uint64_t hash = UINT64_C(0xcbf29ce484222325);
  for (uint16_t pixel : canvas) {
    hash ^= pixel & 0xff;
    hash *= UINT64_C(0x100000001b3);
    hash ^= pixel >> 8;
    hash *= UINT64_C(0x100000001b3);
  }
  return hash;
}

struct ColorBounds {
  bool found;
  int left;
  int top;
  int right;
  int bottom;
};

ColorBounds find_color_bounds(const Canvas &canvas, uint16_t color,
                              int top, int bottom) {
  ColorBounds bounds = {false, kLogicalWidth, kLogicalHeight, -1, -1};
  for (int y = top; y < bottom; ++y) {
    for (int x = 0; x < kLogicalWidth; ++x) {
      if (canvas[static_cast<size_t>(y) * kLogicalWidth + x] != color)
        continue;
      bounds.found = true;
      bounds.left = std::min(bounds.left, x);
      bounds.top = std::min(bounds.top, y);
      bounds.right = std::max(bounds.right, x);
      bounds.bottom = std::max(bounds.bottom, y);
    }
  }
  return bounds;
}

int color_span_on_row(const Canvas &canvas, uint16_t color, int y) {
  int left = kLogicalWidth;
  int right = -1;
  for (int x = 0; x < kLogicalWidth; ++x) {
    if (canvas[static_cast<size_t>(y) * kLogicalWidth + x] == color) {
      left = std::min(left, x);
      right = std::max(right, x);
    }
  }
  return right >= left ? right - left + 1 : 0;
}

} // namespace

int main(int argc, char **argv) {
  assert(argc == 2);
  std::vector<ProjectCredit> credits;
  std::string error;
  assert(load_project_credits(argv[1], &credits, &error));
  assert(credits.size() >= 25);

  std::set<std::string> projects;
  for (size_t index = 0; index < credits.size(); ++index) {
    assert(!credits[index].project.empty());
    assert(!credits[index].role.empty());
    assert(!credits[index].license.empty());
    assert(projects.insert(credits[index].project).second);
  }
  assert(projects.count("FCEUmm") == 1);
  assert(projects.count("Gambatte") == 1);
  assert(projects.count("Fuse") == 1);
  assert(projects.count("c-octo") == 1);

  const CreditsCrawl crawl = make_project_credits_crawl(credits);
  assert(!crawl.lines.empty());
  assert(crawl.static_lines.size() == credits.size());
  assert(crawl.content_height > 0);
  for (size_t index = 0; index < crawl.lines.size(); ++index) {
    assert(crawl.lines[index].source_width ==
           text_width(crawl.lines[index].text, kCreditsCrawlTextScale));
    assert(crawl.lines[index].source_width < kLogicalWidth);
    assert(crawl.lines[index].source_height ==
           kBitmapGlyphHeight * kCreditsCrawlTextScale);
    assert(crawl.lines[index].pixels.size() ==
           static_cast<size_t>(crawl.lines[index].source_width *
                               crawl.lines[index].source_height));
  }

  const uint16_t reference_background = rgb565(0x000000);
  const uint16_t reference_accent = rgb565(0xffffaf);
  const uint16_t reference_text = rgb565(0xeeeeee);
  const uint16_t reference_muted = rgb565(0x949494);
  const auto reference_hash = [&](bool reduced_motion, int64_t elapsed_ms) {
    Canvas frame;
    CreditsLayout frame_layout;
    render_project_credits(crawl, reduced_motion, elapsed_ms,
                           reference_background, reference_accent,
                           reference_text, reference_muted, &frame,
                           &frame_layout);
    return canvas_hash(frame);
  };
  assert(reference_hash(false, 0) == UINT64_C(0x94ebf079be6e596b));
  assert(reference_hash(false, 2000) == UINT64_C(0x1f14f6b786549363));
  assert(reference_hash(false, 20000) == UINT64_C(0x6267b51f6f787c83));
  assert(reference_hash(true, 0) == UINT64_C(0x9a44bcef4a13dde3));
  assert(reference_hash(true, 60000) == UINT64_C(0x9a44bcef4a13dde3));

  Canvas first;
  Canvas second;
  CreditsLayout first_layout;
  CreditsLayout second_layout;
  render_project_credits(crawl, false, 0, 0, 0xfd20, 0xffff, 0x7bef,
                         &first, &first_layout);
  render_project_credits(crawl, false, 2000, 0, 0xfd20, 0xffff, 0x7bef,
                         &second, &second_layout);
  assert(first.size() == static_cast<size_t>(kLogicalWidth * kLogicalHeight));
  assert(first != second);
  assert(first_layout.close_button.contains(1240, 40));
  assert(credits_target_at(first_layout, 1240, 40) == CreditsTargetClose);
  assert(credits_target_at(first_layout, 600, 240) == CreditsTargetNone);

  const uint16_t accent = 0xfd20;
  std::vector<ProjectCredit> perspective_fixture;
  perspective_fixture.push_back(
      ProjectCredit{"HHHHHHHHHH", "ROLE", "MIT"});
  const CreditsCrawl fixture_crawl =
      make_project_credits_crawl(perspective_fixture);
  CreditsCrawl single_line;
  for (size_t index = 0; index < fixture_crawl.lines.size(); ++index) {
    if (fixture_crawl.lines[index].text == "HHHHHHHHHH") {
      single_line.lines.push_back(fixture_crawl.lines[index]);
      single_line.lines.back().source_y = 0;
      break;
    }
  }
  assert(single_line.lines.size() == 1);
  single_line.content_height = 44;

  Canvas near_frame;
  Canvas far_frame;
  CreditsLayout single_layout;
  render_project_credits(single_line, false, 1000, 0, accent, 0xffff,
                         0x7bef, &near_frame, &single_layout);
  render_project_credits(single_line, false, 3000, 0, accent, 0xffff,
                         0x7bef, &far_frame, &single_layout);
  const ColorBounds near_bounds =
      find_color_bounds(near_frame, accent, 80, 458);
  const ColorBounds far_bounds =
      find_color_bounds(far_frame, accent, 80, 458);
  assert(near_bounds.found && far_bounds.found);
  assert(far_bounds.top < near_bounds.top);
  assert(far_bounds.right - far_bounds.left <
         near_bounds.right - near_bounds.left);
  assert(color_span_on_row(near_frame, accent, near_bounds.bottom) >
         color_span_on_row(near_frame, accent, near_bounds.top));

  std::set<int> projected_widths;
  for (int64_t elapsed = 1000; elapsed <= 3000; elapsed += 100) {
    Canvas frame;
    render_project_credits(single_line, false, elapsed, 0, accent, 0xffff,
                           0x7bef, &frame, &single_layout);
    const ColorBounds bounds = find_color_bounds(frame, accent, 80, 458);
    assert(bounds.found);
    projected_widths.insert(bounds.right - bounds.left + 1);
  }
  assert(projected_widths.size() >= 10);

  Canvas previous_frame;
  size_t changed_frames = 0;
  for (int frame_number = 0; frame_number < 120; ++frame_number) {
    Canvas frame;
    render_project_credits(crawl, false, 1000 + frame_number * 16, 0,
                           accent, 0xffff, 0x7bef, &frame, &single_layout);
    if (!previous_frame.empty() && frame != previous_frame)
      ++changed_frames;
    previous_frame.swap(frame);
  }
  assert(changed_frames >= 80);

  Canvas static_first;
  Canvas static_second;
  render_project_credits(crawl, true, 0, 0, accent, 0xffff, 0x7bef,
                         &static_first, &single_layout);
  render_project_credits(crawl, true, 60000, 0, accent, 0xffff, 0x7bef,
                         &static_second, &single_layout);
  assert(static_first == static_second);

  std::cout << "menu_credits_test: OK\n";
  return 0;
}
