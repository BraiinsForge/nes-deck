#include <cassert>
#include <fstream>
#include <iostream>
#include <string>

#include <cstdlib>
#include <unistd.h>

#include "../src/menu_network.h"

namespace {

void write_file(const std::string &path, const std::string &contents) {
  std::ofstream output(path.c_str(), std::ios::out | std::ios::binary |
                                         std::ios::trunc);
  assert(output);
  output.write(contents.data(), static_cast<std::streamsize>(contents.size()));
  output.close();
  assert(output);
}

} // namespace

int main() {
  char directory_template[] = "/tmp/menu-network-test-XXXXXX";
  char *directory_name = mkdtemp(directory_template);
  assert(directory_name);
  const std::string directory(directory_name);
  const std::string status_path = directory + "/status";
  const std::string link_path = directory + "/status-link";

  write_file(status_path, "CONNECTED TO NET1\n");
  assert(read_wifi_selector_status(status_path) == "CONNECTED TO NET1");

  write_file(status_path, "CONNECTED\r\n");
  assert(read_wifi_selector_status(status_path) == "CONNECTED");

  write_file(status_path, " LEADING SPACE\n");
  assert(read_wifi_selector_status(status_path) == "STATUS INVALID");

  write_file(status_path, std::string("BAD ") + "\xc0\xaf" + "\n");
  assert(read_wifi_selector_status(status_path) == "STATUS INVALID");

  write_file(status_path, std::string(129, 'X'));
  assert(read_wifi_selector_status(status_path) == "STATUS UNAVAILABLE");
  assert(symlink(status_path.c_str(), link_path.c_str()) == 0);
  assert(read_wifi_selector_status(link_path) == "STATUS UNAVAILABLE");
  assert(read_wifi_selector_status("relative/status") ==
         "STATUS UNAVAILABLE");

  NetworkStatus first;
  first.ssid = "NET1";
  first.wlan_ipv4 = "10.0.1.11";
  first.wireguard_ipv4 = "10.0.0.15";
  first.selector = "CONNECTED";
  NetworkStatus same = first;
  NetworkStatus different = first;
  different.selector = "RECOVERING";
  assert(first == same);
  assert(first != different);

  assert(interface_ipv4(NULL).empty());
  assert(interface_ipv4("").empty());
  assert(interface_ipv4("retro-deck-missing").empty());
  assert(wireless_ssid(NULL).empty());
  assert(wireless_ssid("retro-deck-missing").empty());

  assert(unlink(link_path.c_str()) == 0);
  assert(unlink(status_path.c_str()) == 0);
  assert(rmdir(directory.c_str()) == 0);

  std::cout << "menu_network_test: OK\n";
  return 0;
}
