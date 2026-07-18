#ifndef RETRO_DECK_MENU_STATE_H
#define RETRO_DECK_MENU_STATE_H

#include <string>

const unsigned int kBrightnessStep = 10;
const unsigned int kMinimumBrightness = 10;

bool parse_volume_percent(const std::string &text, unsigned int *volume);
bool save_volume_state(const std::string &path, unsigned int volume,
                       std::string *error);
bool load_volume_state(const std::string &path, unsigned int default_volume,
                       unsigned int *volume, std::string *error);

unsigned int brightness_raw_value(unsigned int percent, unsigned int maximum);
bool set_brightness_percent(const std::string &brightness_path,
                            const std::string &state_path,
                            unsigned int maximum, unsigned int percent,
                            std::string *error);
bool load_brightness(const std::string &brightness_path,
                     const std::string &maximum_path,
                     const std::string &state_path, unsigned int *maximum,
                     unsigned int *percent, std::string *error);

bool valid_keymap(const std::string &keymap);
bool save_keymap_state(const std::string &path, const std::string &keymap,
                       std::string *error);
bool load_keymap_state(const std::string &path, std::string *keymap,
                       std::string *error);

#endif
