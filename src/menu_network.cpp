#include "menu_network.h"

#include "menu_text.h"

#include <algorithm>
#include <arpa/inet.h>
#include <cstring>
#include <fstream>
#include <ifaddrs.h>
#include <linux/wireless.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

bool NetworkStatus::operator==(const NetworkStatus &other) const {
  return ssid == other.ssid && wlan_ipv4 == other.wlan_ipv4 &&
         wireguard_ipv4 == other.wireguard_ipv4 &&
         selector == other.selector;
}

bool NetworkStatus::operator!=(const NetworkStatus &other) const {
  return !(*this == other);
}

std::string interface_ipv4(const char *interface_name) {
  if (!interface_name || !*interface_name)
    return std::string();
  struct ifaddrs *addresses = NULL;
  if (getifaddrs(&addresses) != 0)
    return std::string();
  std::string result;
  for (const struct ifaddrs *entry = addresses; entry; entry = entry->ifa_next) {
    if (!entry->ifa_addr || !entry->ifa_name ||
        std::strcmp(entry->ifa_name, interface_name) != 0 ||
        entry->ifa_addr->sa_family != AF_INET)
      continue;
    char text[INET_ADDRSTRLEN] = {};
    const struct sockaddr_in *address =
        reinterpret_cast<const struct sockaddr_in *>(entry->ifa_addr);
    if (inet_ntop(AF_INET, &address->sin_addr, text, sizeof(text))) {
      result = text;
      break;
    }
  }
  freeifaddrs(addresses);
  return result;
}

std::string wireless_ssid(const char *interface_name) {
  if (!interface_name || !*interface_name)
    return std::string();
  const int socket_fd = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
  if (socket_fd < 0)
    return std::string();
  struct iwreq request;
  std::memset(&request, 0, sizeof(request));
  std::strncpy(request.ifr_name, interface_name, IFNAMSIZ - 1);
  char value[IW_ESSID_MAX_SIZE + 1] = {};
  request.u.essid.pointer = value;
  request.u.essid.length = IW_ESSID_MAX_SIZE;
  request.u.essid.flags = 0;
  const bool read = ioctl(socket_fd, SIOCGIWESSID, &request) == 0;
  close(socket_fd);
  if (!read)
    return std::string();
  const size_t length =
      std::min<size_t>(request.u.essid.length, IW_ESSID_MAX_SIZE);
  std::string ssid(value, value + length);
  while (!ssid.empty() && ssid[ssid.size() - 1] == '\0')
    ssid.erase(ssid.size() - 1);
  return valid_utf8_text(ssid, 32, true) ? display_ascii(ssid)
                                         : std::string("?");
}

std::string read_wifi_selector_status(const std::string &path) {
  if (!is_absolute_path(path))
    return "STATUS UNAVAILABLE";
  struct stat info;
  if (lstat(path.c_str(), &info) != 0 || !S_ISREG(info.st_mode) ||
      info.st_size < 1 || info.st_size > 128)
    return "STATUS UNAVAILABLE";
  std::ifstream input(path.c_str(), std::ios::in | std::ios::binary);
  std::string line;
  if (!input || !std::getline(input, line) || input.bad())
    return "STATUS UNAVAILABLE";
  if (!line.empty() && line[line.size() - 1] == '\r')
    line.erase(line.size() - 1);
  if (trim_ascii_space(line) != line || !valid_utf8_text(line, 64, false))
    return "STATUS INVALID";
  return display_ascii(line);
}

NetworkStatus read_network_status(const std::string &selector_status_path) {
  NetworkStatus status;
  status.ssid = wireless_ssid("wlan0");
  status.wlan_ipv4 = interface_ipv4("wlan0");
  status.wireguard_ipv4 = interface_ipv4("wg0");
  status.selector = read_wifi_selector_status(selector_status_path);
  return status;
}
