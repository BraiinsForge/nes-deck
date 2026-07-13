#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#include <vector>

#include "../src/nes_sram.h"

namespace {

void expect(bool condition, const char *message) {
  if (!condition) {
    fprintf(stderr, "FAIL: %s\n", message);
    exit(1);
  }
}

void expect_round_trip(const std::vector<uint8_t> &input,
                       const char *message) {
  std::vector<uint8_t> encoded(NesSramMaximumEncodedSize(input.size()));
  size_t encoded_size = 0;
  expect(NesSramEncode(input.data(), input.size(), encoded.data(),
                       encoded.size(), &encoded_size),
         "SRAM encoding succeeds");
  encoded.resize(encoded_size);
  std::vector<uint8_t> decoded(input.size());
  expect(NesSramDecode(encoded.data(), encoded.size(), decoded.data(),
                       decoded.size()),
         "SRAM decoding succeeds");
  expect(decoded == input, message);
}

} // namespace

int main() {
  std::vector<uint8_t> zeros(8192, 0);
  expect_round_trip(zeros, "zeroed cartridge RAM round trips");

  std::vector<uint8_t> changing(8192);
  for (size_t index = 0; index < changing.size(); ++index)
    changing[index] = static_cast<uint8_t>((index * 73 + index / 19) & 0xff);
  expect_round_trip(changing, "dense cartridge RAM round trips");

  std::vector<uint8_t> runs(8192);
  for (size_t index = 0; index < runs.size(); ++index)
    runs[index] = static_cast<uint8_t>((index / 257) & 0xff);
  expect_round_trip(runs, "long SRAM runs round trip");

  const uint8_t truncated[] = {0, 0, 42};
  uint8_t output[8] = {};
  expect(!NesSramDecode(truncated, sizeof(truncated), output, sizeof(output)),
         "truncated run is rejected");

  const uint8_t oversized_run[] = {0, 0, 42, 8};
  expect(!NesSramDecode(oversized_run, sizeof(oversized_run), output,
                        sizeof(output)),
         "oversized run is rejected");

  puts("nes_sram_test: OK");
  return 0;
}
