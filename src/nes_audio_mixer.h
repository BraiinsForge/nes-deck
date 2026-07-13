#ifndef NES_AUDIO_MIXER_H
#define NES_AUDIO_MIXER_H

#include <stddef.h>
#include <stdint.h>

/*
 * InfoNES does not return five equally scaled PCM streams:
 *
 *   pulse 1/2, triangle: 0..255
 *   noise:               0..15
 *   DPCM:                0..127
 *
 * Convert those ranges back to the APU's 4/4/4/4/7-bit DAC levels, then use
 * the standard nonlinear pulse and triangle/noise/DPCM lookup equations.
 * The resulting signal is unipolar, just like the NES DAC, so a small
 * fixed-point DC blocker converts it to signed 16-bit PCM.  Keeping the
 * final signal at the Deck's native precision matters here: the previous
 * AFMT_U8 path reduced normal game audio to only a few dozen distinct levels
 * before ALSA expanded it back to 16-bit, which made quiet passages sound
 * conspicuously grainy.
 */
struct NesAudioMixer {
  int32_t dc_q16;
  int32_t output_scale;
  uint16_t pulse_q14[31];
  uint16_t tnd_q14[203];
};

static inline void NesAudioMixer_Reset(NesAudioMixer *mixer) {
  mixer->dc_q16 = 0;
  mixer->output_scale = 12042;
  mixer->pulse_q14[0] = 0;
  for (unsigned int i = 1; i < 31; ++i) {
    mixer->pulse_q14[i] = (uint16_t)(
        ((uint64_t)9588 * 16384 * i) / (100 * (8128 + 100 * i)));
  }
  mixer->tnd_q14[0] = 0;
  for (unsigned int i = 1; i < 203; ++i) {
    mixer->tnd_q14[i] = (uint16_t)(
        ((uint64_t)16367 * 16384 * i) / (100 * (24329 + 100 * i)));
  }
}

static inline void NesAudioMixer_SetVolumePercent(NesAudioMixer *mixer,
                                                  unsigned int percent) {
  if (percent > 100)
    percent = 100;
  /* 112 * 256 is the effective full-scale gain of the former U8 path. */
  mixer->output_scale = (int32_t)((28672U * percent + 50U) / 100U);
}

static inline int16_t NesAudioMixer_MixSampleS16(NesAudioMixer *mixer,
                                                 uint8_t pulse1,
                                                 uint8_t pulse2,
                                                 uint8_t triangle,
                                                 uint8_t noise,
                                                 uint8_t dpcm) {
  unsigned int p1 = ((unsigned int)pulse1 + 8) / 17;
  unsigned int p2 = ((unsigned int)pulse2 + 8) / 17;
  unsigned int tri = (unsigned int)triangle >> 4;
  unsigned int ns = noise & 0x0f;
  unsigned int dmc = dpcm & 0x7f;
  if (p1 > 15)
    p1 = 15;
  if (p2 > 15)
    p2 = 15;

  const unsigned int pulse_index = p1 + p2;
  const unsigned int tnd_index = 3 * tri + 2 * ns + dmc;
  const int raw_q14 =
      (int)mixer->pulse_q14[pulse_index] + mixer->tnd_q14[tnd_index];
  const int32_t target_q16 = (int32_t)(raw_q14 << 16);

  /* About a 7 Hz corner at 44.1 kHz: remove DC without eating bass. */
  mixer->dc_q16 += (target_q16 - mixer->dc_q16) >> 10;

  const int centered_q14 = raw_q14 - (mixer->dc_q16 >> 16);
  /*
   * The default scale of 12042 leaves comfortable headroom and is about 42%
   * of the effective gain of the old U8 path.  The MAX98357A has no
   * hardware volume control and /dev/dsp bypasses ALSA's userspace softvol,
   * so gain belongs here.
   */
  int sample = centered_q14 * mixer->output_scale / 16384;
  if (sample < -32768)
    sample = -32768;
  else if (sample > 32767)
    sample = 32767;

  return (int16_t)sample;
}

/*
 * The Deck's OSS compatibility layer currently accepts 44.1 kHz exactly,
 * but SNDCTL_DSP_SPEED is allowed to coerce it.  This allocation helper and
 * small linear resampler keep pitch and frame pacing correct if that happens.
 */
static inline size_t NesAudio_ResampledCapacity(size_t input_samples,
                                                unsigned int input_rate,
                                                unsigned int output_rate) {
  if (!input_samples || !input_rate || !output_rate)
    return 0;
  return (input_samples * (size_t)output_rate + input_rate - 1) / input_rate +
         1;
}

static inline void NesAudio_ResampleS16(const int16_t *input,
                                        size_t input_samples, int16_t *output,
                                        size_t output_samples) {
  if (!input || !output || !input_samples || !output_samples)
    return;

  if (input_samples == 1 || output_samples == 1) {
    for (size_t i = 0; i < output_samples; ++i)
      output[i] = input[0];
    return;
  }

  const uint32_t step_q16 =
      (uint32_t)(((input_samples - 1) << 16) / (output_samples - 1));
  uint32_t position_q16 = 0;

  for (size_t i = 0; i < output_samples; ++i) {
    if (i == output_samples - 1) {
      output[i] = input[input_samples - 1];
      break;
    }
    size_t index = position_q16 >> 16;
    uint32_t fraction = position_q16 & 0xffff;
    if (index >= input_samples - 1) {
      output[i] = input[input_samples - 1];
    } else {
      const int first = input[index];
      const int delta = (int)input[index + 1] - first;
      output[i] = (int16_t)(first + ((delta * (int)fraction) >> 16));
    }
    position_q16 += step_q16;
  }
}

#endif /* NES_AUDIO_MIXER_H */
