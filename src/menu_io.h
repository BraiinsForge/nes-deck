#ifndef RETRO_DECK_MENU_IO_H
#define RETRO_DECK_MENU_IO_H

#include <cstddef>
#include <string>

std::string errno_message(const std::string &what);
bool write_all(int fd, const char *data, size_t size);

#endif
