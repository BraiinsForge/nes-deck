#ifndef NES_DECK_NES_SRAM_H
#define NES_DECK_NES_SRAM_H

#include <stddef.h>
#include <stdint.h>

inline size_t NesSramMaximumEncodedSize(size_t input_size) {
  return input_size * 3 + 1;
}

inline bool NesSramEncode(const uint8_t *input, size_t input_size,
                          uint8_t *output, size_t output_capacity,
                          size_t *output_size) {
  if (!input || input_size == 0 || !output || !output_size ||
      output_capacity == 0)
    return false;

  size_t frequency[256] = {};
  for (size_t index = 0; index < input_size; ++index)
    ++frequency[input[index]];

  uint8_t tag = 0;
  for (size_t value = 1; value < 256; ++value) {
    if (frequency[value] < frequency[tag])
      tag = static_cast<uint8_t>(value);
  }

  size_t written = 0;
  output[written++] = tag;
  size_t input_index = 0;
  while (input_index < input_size) {
    const uint8_t value = input[input_index];
    size_t run_length = 1;
    while (input_index + run_length < input_size && run_length < 256 &&
           input[input_index + run_length] == value)
      ++run_length;

    if (run_length >= 4 || value == tag) {
      if (output_capacity - written < 3)
        return false;
      output[written++] = tag;
      output[written++] = value;
      output[written++] = static_cast<uint8_t>(run_length - 1);
    } else {
      if (output_capacity - written < run_length)
        return false;
      for (size_t index = 0; index < run_length; ++index)
        output[written++] = value;
    }
    input_index += run_length;
  }

  *output_size = written;
  return true;
}

inline bool NesSramDecode(const uint8_t *input, size_t input_size,
                          uint8_t *output, size_t output_size) {
  if (!input || input_size == 0 || !output || output_size == 0)
    return false;

  const uint8_t tag = input[0];
  size_t input_index = 1;
  size_t output_index = 0;
  while (output_index < output_size) {
    if (input_index >= input_size)
      return false;
    const uint8_t value = input[input_index++];
    if (value != tag) {
      output[output_index++] = value;
      continue;
    }
    if (input_size - input_index < 2)
      return false;
    const uint8_t repeated_value = input[input_index++];
    const size_t run_length = static_cast<size_t>(input[input_index++]) + 1;
    if (output_size - output_index < run_length)
      return false;
    for (size_t index = 0; index < run_length; ++index)
      output[output_index++] = repeated_value;
  }
  return true;
}

#endif
