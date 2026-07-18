#include <cassert>
#include <fstream>
#include <iostream>
#include <iterator>
#include <string>

#include <cstdlib>
#include <sys/stat.h>
#include <unistd.h>

#include "../src/menu_state.h"

namespace {

void write_file(const std::string &path, const std::string &contents) {
  std::ofstream output(path.c_str(), std::ios::out | std::ios::binary |
                                         std::ios::trunc);
  assert(output);
  output.write(contents.data(), static_cast<std::streamsize>(contents.size()));
  output.close();
  assert(output);
}

std::string read_file(const std::string &path) {
  std::ifstream input(path.c_str(), std::ios::in | std::ios::binary);
  return std::string(std::istreambuf_iterator<char>(input),
                     std::istreambuf_iterator<char>());
}

} // namespace

int main() {
  char directory_template[] = "/tmp/menu-state-test-XXXXXX";
  char *directory_name = mkdtemp(directory_template);
  assert(directory_name);
  const std::string directory(directory_name);
  const std::string volume_path = directory + "/volume.state";
  const std::string brightness_path = directory + "/brightness";
  const std::string maximum_path = directory + "/max_brightness";
  const std::string brightness_state_path = directory + "/brightness.state";
  const std::string keymap_path = directory + "/keymap.state";
  std::string error;

  unsigned int parsed = 999;
  assert(parse_volume_percent("0", &parsed) && parsed == 0);
  assert(parse_volume_percent("100", &parsed) && parsed == 100);
  assert(!parse_volume_percent("042", &parsed));
  assert(!parse_volume_percent("101", &parsed));
  assert(!parse_volume_percent("", &parsed));

  unsigned int volume = 0;
  assert(load_volume_state(volume_path, 42, &volume, &error));
  assert(volume == 42 && read_file(volume_path) == "42\n");
  struct stat volume_info;
  assert(stat(volume_path.c_str(), &volume_info) == 0);
  assert((volume_info.st_mode & 077) == 0);

  write_file(volume_path, "on\n");
  assert(load_volume_state(volume_path, 37, &volume, &error));
  assert(volume == 37 && read_file(volume_path) == "37\n");
  write_file(volume_path, "off\n");
  assert(load_volume_state(volume_path, 37, &volume, &error));
  assert(volume == 0 && read_file(volume_path) == "0\n");
  write_file(volume_path, "042\n");
  assert(!load_volume_state(volume_path, 42, &volume, &error));
  assert(!save_volume_state("relative.state", 42, &error));
  assert(!save_volume_state(volume_path, 101, &error));

  write_file(brightness_path, "12\n");
  write_file(maximum_path, "20\n");
  unsigned int maximum = 0;
  unsigned int brightness = 0;
  assert(load_brightness(brightness_path, maximum_path, brightness_state_path,
                         &maximum, &brightness, &error));
  assert(maximum == 20 && brightness == 60);
  assert(read_file(brightness_path) == "12\n");
  assert(read_file(brightness_state_path) == "60\n");
  assert(brightness_raw_value(10, 20) == 2);
  assert(brightness_raw_value(60, 20) == 12);
  assert(brightness_raw_value(100, 20) == 20);
  assert(set_brightness_percent(brightness_path, brightness_state_path, 20,
                                70, &error));
  assert(read_file(brightness_path) == "14\n");
  assert(read_file(brightness_state_path) == "70\n");
  assert(!set_brightness_percent(brightness_path, brightness_state_path, 20,
                                 65, &error));

  std::string keymap;
  assert(load_keymap_state(keymap_path, &keymap, &error));
  assert(keymap == "us" && read_file(keymap_path) == "us\n");
  assert(save_keymap_state(keymap_path, "cz", &error));
  assert(load_keymap_state(keymap_path, &keymap, &error));
  assert(keymap == "cz");
  assert(!save_keymap_state(keymap_path, "de", &error));
  write_file(keymap_path, "de\n");
  assert(!load_keymap_state(keymap_path, &keymap, &error));

  assert(unlink(keymap_path.c_str()) == 0);
  assert(unlink(brightness_state_path.c_str()) == 0);
  assert(unlink(maximum_path.c_str()) == 0);
  assert(unlink(brightness_path.c_str()) == 0);
  assert(unlink(volume_path.c_str()) == 0);
  assert(rmdir(directory.c_str()) == 0);

  std::cout << "menu_state_test: OK\n";
  return 0;
}
