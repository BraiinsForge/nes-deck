#include <assert.h>
#include <stdint.h>
#include <stdio.h>

#include "nes_audio_mixer.h"

static int16_t first_sample(uint8_t pulse1, uint8_t pulse2, uint8_t triangle,
                            uint8_t noise, uint8_t dpcm) {
  NesAudioMixer mixer;
  NesAudioMixer_Reset(&mixer);
  return NesAudioMixer_MixSampleS16(&mixer, pulse1, pulse2, triangle, noise,
                                    dpcm);
}

static void test_silence_is_zero(void) {
  NesAudioMixer mixer;
  NesAudioMixer_Reset(&mixer);
  for (int i = 0; i < 4096; ++i)
    assert(NesAudioMixer_MixSampleS16(&mixer, 0, 0, 0, 0, 0) == 0);
}

static void test_nonlinear_dac_mix_has_headroom(void) {
  const int pulse = first_sample(255, 0, 0, 0, 0);
  const int triangle = first_sample(0, 0, 255, 0, 0);
  const int noise = first_sample(0, 0, 0, 15, 0);
  const int dpcm = first_sample(0, 0, 0, 0, 127);
  const int everything = first_sample(255, 255, 255, 15, 127);

  assert(pulse > 0);
  assert(triangle > 0);
  assert(noise > 0);
  assert(dpcm > 0);
  assert(everything > dpcm);
  assert(everything < 32767);
}

static void test_s16_path_preserves_dac_resolution(void) {
  int distinct = 0;
  int16_t previous = first_sample(0, 0, 0, 0, 0);
  for (int dpcm = 1; dpcm <= 127; ++dpcm) {
    const int16_t sample = first_sample(0, 0, 0, 0, (uint8_t)dpcm);
    if (sample != previous)
      ++distinct;
    previous = sample;
  }

  /* The old U8 endpoint collapsed this sweep to only a few dozen values. */
  assert(distinct >= 100);
}

static void test_volume_control(void) {
  NesAudioMixer full;
  NesAudioMixer quiet;
  NesAudioMixer_Reset(&full);
  NesAudioMixer_Reset(&quiet);
  NesAudioMixer_SetVolumePercent(&full, 100);
  NesAudioMixer_SetVolumePercent(&quiet, 25);

  const int full_sample =
      NesAudioMixer_MixSampleS16(&full, 255, 0, 0, 0, 0);
  const int quiet_sample =
      NesAudioMixer_MixSampleS16(&quiet, 255, 0, 0, 0, 0);
  assert(full_sample > quiet_sample);
  assert(quiet_sample > 0);
  assert(quiet_sample * 4 >= full_sample - 4);
  assert(quiet_sample * 4 <= full_sample + 4);

  NesAudioMixer_SetVolumePercent(&full, 101);
  assert(full.output_scale == 28672);
}

static void test_dc_blocker_returns_constant_signal_to_silence(void) {
  NesAudioMixer mixer;
  NesAudioMixer_Reset(&mixer);
  int16_t sample = 0;
  for (int i = 0; i < 20000; ++i)
    sample = NesAudioMixer_MixSampleS16(&mixer, 255, 0, 0, 0, 0);
  assert(sample >= -2 && sample <= 2);
}

static void test_linear_resampler(void) {
  const int16_t input[] = {-30000, -15000, 0, 15000, 30000};
  int16_t output[9] = {};
  NesAudio_ResampleS16(input, 5, output, 9);

  assert(output[0] == input[0]);
  assert(output[8] == input[4]);
  for (int i = 1; i < 9; ++i)
    assert(output[i] >= output[i - 1]);

  assert(NesAudio_ResampledCapacity(735, 44100, 48000) >= 800);
}

int main(void) {
  test_silence_is_zero();
  test_nonlinear_dac_mix_has_headroom();
  test_s16_path_preserves_dac_resolution();
  test_volume_control();
  test_dc_blocker_returns_constant_signal_to_silence();
  test_linear_resampler();
  puts("nes_audio_test: OK");
  return 0;
}
