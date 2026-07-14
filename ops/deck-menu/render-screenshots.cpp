#include <cerrno>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <sstream>

#define main deck_menu_embedded_main
#include "../../src/deck_menu.cpp"
#undef main

namespace {

bool read_catalog(const std::string &path, std::vector<GameEntry> *games,
                  std::string *error) {
  if (!games)
    return false;
  std::ifstream input(path.c_str());
  if (!input) {
    if (error)
      *error = "cannot open catalog " + path;
    return false;
  }
  games->clear();
  std::string line;
  size_t line_number = 0;
  while (std::getline(input, line)) {
    ++line_number;
    if (!line.empty() && line[line.size() - 1] == '\r')
      line.resize(line.size() - 1);
    if (line.empty() || line[0] == '#')
      continue;
    const std::vector<std::string> fields = split_tabs(line);
    GameEntry game;
    if (fields.size() != 5 || fields[0].empty() || fields[1].empty() ||
        fields[2].empty() || fields[3].empty() ||
        !parse_color(fields[4], &game.color)) {
      if (error) {
        *error = "invalid catalog row " + std::to_string(line_number) +
                 " in " + path;
      }
      return false;
    }
    game.id = fields[0];
    game.title = fields[1];
    game.system = fields[2];
    game.rom = fields[3];
    games->push_back(game);
  }
  return !games->empty();
}

bool write_canvas_png(const std::string &path, const Canvas &canvas,
                      std::string *error) {
  if (canvas.size() != static_cast<size_t>(kLogicalWidth * kLogicalHeight)) {
    if (error)
      *error = "renderer returned an invalid canvas";
    return false;
  }
  std::vector<png_byte> rgb(canvas.size() * 3);
  for (size_t i = 0; i < canvas.size(); ++i) {
    const uint16_t pixel = canvas[i];
    const unsigned int red = (pixel >> 11) & 0x1f;
    const unsigned int green = (pixel >> 5) & 0x3f;
    const unsigned int blue = pixel & 0x1f;
    rgb[i * 3] = static_cast<png_byte>((red << 3) | (red >> 2));
    rgb[i * 3 + 1] =
        static_cast<png_byte>((green << 2) | (green >> 4));
    rgb[i * 3 + 2] = static_cast<png_byte>((blue << 3) | (blue >> 2));
  }

  png_image image;
  std::memset(&image, 0, sizeof(image));
  image.version = PNG_IMAGE_VERSION;
  image.width = kLogicalWidth;
  image.height = kLogicalHeight;
  image.format = PNG_FORMAT_RGB;
  if (!png_image_write_to_file(&image, path.c_str(), 0, &rgb[0], 0, NULL)) {
    if (error)
      *error = "cannot write " + path + ": " + image.message;
    return false;
  }
  return true;
}

std::string numbered_name(size_t number, const std::string &label) {
  std::ostringstream name;
  name << std::setw(2) << std::setfill('0') << number << '-' << label
       << ".png";
  return name.str();
}

bool save_canvas(const std::string &directory, size_t number,
                 const std::string &label, const Canvas &canvas,
                 std::string *error) {
  return write_canvas_png(directory + "/" + numbered_name(number, label),
                          canvas, error);
}

} // namespace

int main(int argc, char **argv) {
  if (argc != 4) {
    std::fprintf(stderr,
                 "Usage: %s CATALOG.tsv COVER-DIRECTORY OUTPUT-DIRECTORY\n",
                 argv[0]);
    return 2;
  }
  const std::string catalog(argv[1]);
  const std::string covers(argv[2]);
  const std::string output(argv[3]);
  if (mkdir(output.c_str(), 0755) != 0 && errno != EEXIST) {
    std::fprintf(stderr, "cannot create %s: %s\n", output.c_str(),
                 std::strerror(errno));
    return 1;
  }

  std::string error;
  std::vector<GameEntry> games;
  if (!read_catalog(catalog, &games, &error)) {
    std::fprintf(stderr, "%s\n", error.c_str());
    return 1;
  }
  games.push_back(built_in_terminal_entry("/terminal"));
  games.push_back(built_in_reboot_entry("/sbin/reboot"));
  const size_t cover_count = load_game_covers(covers, &games);
  std::fprintf(stderr, "render-screenshots: loaded %zu covers\n", cover_count);

  Canvas canvas;
  MenuLayout menu_layout;
  size_t number = 1;
  for (size_t definition = 0;
       definition < sizeof(kSystemDefinitions) / sizeof(kSystemDefinitions[0]);
       ++definition) {
    const std::string system(kSystemDefinitions[definition].system);
    if (!has_system(games, system))
      continue;
    render_menu(games, system, 42, "us", false, 0, std::string(), &canvas,
                &menu_layout);
    if (!save_canvas(output, number++, "console-" + system, canvas, &error)) {
      std::fprintf(stderr, "%s\n", error.c_str());
      return 1;
    }
  }

  for (size_t definition = 0;
       definition < sizeof(kSystemDefinitions) / sizeof(kSystemDefinitions[0]);
       ++definition) {
    const std::string system(kSystemDefinitions[definition].system);
    size_t position = 0;
    for (size_t game = 0; game < games.size(); ++game) {
      if (games[game].system != system)
        continue;
      render_menu(games, system, 42, "us", true, position++, std::string(),
                  &canvas, &menu_layout);
      if (!save_canvas(output, number++, "game-" + games[game].id, canvas,
                       &error)) {
        std::fprintf(stderr, "%s\n", error.c_str());
        return 1;
      }
    }
  }

  render_menu(games, "nes", 0, "us", true, 0, std::string(), &canvas,
              &menu_layout);
  if (!save_canvas(output, number++, "volume-off", canvas, &error))
    return 1;
  render_menu(games, "nes", 42, "cz", true, 0, std::string(), &canvas,
              &menu_layout);
  if (!save_canvas(output, number++, "czech-keymap", canvas, &error))
    return 1;
  render_menu(games, "deck", 42, "us", true, 2,
              kRebootConfirmationText, &canvas, &menu_layout);
  if (!save_canvas(output, number++, "reboot-confirmation", canvas, &error))
    return 1;

  WifiState wifi;
  WifiLayout wifi_layout;
  render_wifi(wifi, &canvas, &wifi_layout);
  if (!save_canvas(output, number++, "wifi-lowercase", canvas, &error))
    return 1;
  wifi.uppercase = true;
  render_wifi(wifi, &canvas, &wifi_layout);
  if (!save_canvas(output, number++, "wifi-uppercase", canvas, &error))
    return 1;
  wifi.field = WifiPassphrase;
  wifi.ssid = "NETWORK";
  wifi.passphrase = "password";
  render_wifi(wifi, &canvas, &wifi_layout);
  if (!save_canvas(output, number++, "wifi-password", canvas, &error))
    return 1;
  wifi.symbols = true;
  wifi.uppercase = false;
  render_wifi(wifi, &canvas, &wifi_layout);
  if (!save_canvas(output, number++, "wifi-symbols", canvas, &error))
    return 1;

  std::printf("%zu\n", number);
  return 0;
}
