#ifndef RETRO_DECK_MENU_NETWORK_H
#define RETRO_DECK_MENU_NETWORK_H

#include <string>

struct NetworkStatus {
  std::string ssid;
  std::string wlan_ipv4;
  std::string wireguard_ipv4;
  std::string selector;

  bool operator==(const NetworkStatus &other) const;
  bool operator!=(const NetworkStatus &other) const;
};

std::string interface_ipv4(const char *interface_name);
std::string wireless_ssid(const char *interface_name);
std::string read_wifi_selector_status(const std::string &path);
NetworkStatus read_network_status(const std::string &selector_status_path);

#endif
