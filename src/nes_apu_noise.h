#ifndef NES_APU_NOISE_H
#define NES_APU_NOISE_H

#include <stdint.h>

/* Clock the NES APU's 15-bit noise shift register once. */
static inline uint32_t NesApu_ClockNoise(uint32_t lfsr, int short_mode) {
  const unsigned int tap = short_mode ? 6U : 1U;
  const uint32_t feedback = (lfsr ^ (lfsr >> tap)) & 1U;
  return ((lfsr >> 1) | (feedback << 14)) & 0x7fffU;
}

/*
 * Convert a fixed-point phase increment into an exact number of LFSR clocks.
 * A 64-bit sum makes the helper correct even if a future quality table uses
 * an increment large enough to overflow a 32-bit intermediate.
 */
static inline unsigned int NesApu_NoiseClockCount(uint32_t phase,
                                                  uint32_t increment,
                                                  uint32_t *remainder) {
  const uint64_t total = (uint64_t)phase + increment;
  *remainder = (uint32_t)(total & 0x00ffffffU);
  return (unsigned int)(total >> 24);
}

#endif /* NES_APU_NOISE_H */
