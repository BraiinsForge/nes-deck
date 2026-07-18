#include <cassert>
#include <iostream>
#include <set>

#include "../src/menu_credits.h"

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

  Canvas first;
  Canvas second;
  CreditsLayout first_layout;
  CreditsLayout second_layout;
  render_project_credits(credits, 0, 0, 0xfd20, 0xffff, 0x7bef, &first,
                         &first_layout);
  render_project_credits(credits, 2000, 0, 0xfd20, 0xffff, 0x7bef, &second,
                         &second_layout);
  assert(first.size() == static_cast<size_t>(kLogicalWidth * kLogicalHeight));
  assert(first != second);
  assert(first_layout.close_button.contains(1240, 40));
  assert(credits_target_at(first_layout, 1240, 40) == CreditsTargetClose);
  assert(credits_target_at(first_layout, 600, 240) == CreditsTargetNone);

  std::cout << "menu_credits_test: OK\n";
  return 0;
}
