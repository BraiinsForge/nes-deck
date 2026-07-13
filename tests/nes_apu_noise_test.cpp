#include <assert.h>
#include <stdint.h>
#include <stdio.h>

#include "nes_apu_noise.h"

static void test_long_mode_has_32767_state_period(void) {
  uint32_t lfsr = 1;
  for (unsigned int i = 1; i < 32767; ++i) {
    lfsr = NesApu_ClockNoise(lfsr, 0);
    assert(lfsr != 0);
    assert(lfsr != 1);
  }
  assert(NesApu_ClockNoise(lfsr, 0) == 1);
}

static void test_short_mode_has_93_state_period(void) {
  uint32_t lfsr = 1;
  for (unsigned int i = 1; i < 93; ++i) {
    lfsr = NesApu_ClockNoise(lfsr, 1);
    assert(lfsr != 0);
    assert(lfsr != 1);
  }
  assert(NesApu_ClockNoise(lfsr, 1) == 1);
}

static void test_high_rates_keep_every_clock(void) {
  const uint32_t noise_magic = 0x289d9c00U;
  const uint32_t increment = noise_magic / 4U;
  uint32_t phase = 0;
  uint64_t clocks = 0;

  for (unsigned int sample = 0; sample < 44100; ++sample) {
    uint32_t remainder = 0;
    const unsigned int sample_clocks =
        NesApu_NoiseClockCount(phase, increment, &remainder);
    assert(sample_clocks >= 10 && sample_clocks <= 11);
    clocks += sample_clocks;
    phase = remainder;
  }

  const uint64_t expected = ((uint64_t)increment * 44100U) >> 24;
  assert(clocks == expected);
}

int main(void) {
  test_long_mode_has_32767_state_period();
  test_short_mode_has_93_state_period();
  test_high_rates_keep_every_clock();
  puts("nes_apu_noise_test: OK");
  return 0;
}
