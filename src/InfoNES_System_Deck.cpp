/*===================================================================*/
/*                                                                   */
/*  InfoNES_System_Deck.cpp : Braiins Forge Deck framebuffer port    */
/*                                                                   */
/*  Based on InfoNES_System_Linux.cpp                                */
/*  Modified for landscape display with 90-degree rotation           */
/*                                                                   */
/*===================================================================*/

#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <errno.h>
#include <fcntl.h>
#include <linux/fb.h>
#include <linux/soundcard.h>
#include <signal.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

#include "../InfoNES.h"
#include "../InfoNES_System.h"
#include "../InfoNES_pAPU.h"
#include "nes_audio_mixer.h"

#define TRUE 1
#define FALSE 0

/*-------------------------------------------------------------------*/
/*  Braiins Forge Deck display configuration                         */
/*-------------------------------------------------------------------*/

// Display configuration (same as fbDOOM)
#define FB_ROTATION_90 1   // Portrait LCD used in landscape mode
#define FB_LINE_OFFSET 120 // First 120 lines are invisible
#define FB_OFFSET_X -295   // Center 512 pixels on the 1280-pixel rotated axis
#define FB_OFFSET_Y 0      // Fill the Deck's 480-pixel active panel height

// Effective display dimensions (landscape)
#define DISPLAY_WIDTH 1280
#define DISPLAY_HEIGHT 480

/*-------------------------------------------------------------------*/
/*  Framebuffer variables                                            */
/*-------------------------------------------------------------------*/

static int fb_fd = -1;
static unsigned char *fb_mem = NULL;
static struct fb_var_screeninfo fb_var;
static struct fb_fix_screeninfo fb_fix;
static size_t fb_map_size;
static int fb_bpp;
static int fb_stride;
static int fb_scaling = 1;
static uint16_t *fb_staging = NULL;
static int fb_staging_first_row = 0;
static int fb_staging_rows = 0;
static int fb_staging_columns = 0;
static int fb_vsync_state = 0;
static int runtime_diagnostics = 0;
static uint64_t diagnostic_frames = 0;
static uint64_t diagnostic_render_nanoseconds = 0;
static uint64_t diagnostic_max_render_nanoseconds = 0;
static int64_t diagnostic_window_started = 0;

extern "C" int InitJoypadInput(void);
extern "C" unsigned int GetJoypadInput(unsigned int player);

/*-------------------------------------------------------------------*/
/*  ROM image file information                                       */
/*-------------------------------------------------------------------*/

char szRomName[256];
char szSaveName[256];
int nSRAM_SaveFlag;

/*-------------------------------------------------------------------*/
/*  Global Variables                                                 */
/*-------------------------------------------------------------------*/

pthread_t emulation_tid;
int bThread;
static int emulation_thread_started = 0;

DWORD dwKeyPad1;
DWORD dwKeyPad2;
DWORD dwKeySystem;
static volatile sig_atomic_t shutdown_requested = 0;

static void request_shutdown(int signal_number) {
  (void)signal_number;
  shutdown_requested = 1;
}

static int64_t monotonic_nanoseconds(void) {
  struct timespec now;
  if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
    return 0;
  return (int64_t)now.tv_sec * 1000000000LL + now.tv_nsec;
}

static int environment_flag(const char *name, int default_value) {
  const char *value = getenv(name);
  if (!value)
    return default_value;
  if (strcmp(value, "1") == 0)
    return 1;
  if (strcmp(value, "0") == 0)
    return 0;
  printf("InfoNES: Ignoring invalid %s=%s; expected 0 or 1\n", name, value);
  return default_value;
}

static void record_render_time(int64_t started_at) {
  if (!runtime_diagnostics || started_at <= 0)
    return;
  const int64_t finished_at = monotonic_nanoseconds();
  if (finished_at <= started_at)
    return;
  const uint64_t elapsed = (uint64_t)(finished_at - started_at);
  ++diagnostic_frames;
  diagnostic_render_nanoseconds += elapsed;
  if (elapsed > diagnostic_max_render_nanoseconds)
    diagnostic_max_render_nanoseconds = elapsed;
  if (diagnostic_window_started == 0)
    diagnostic_window_started = started_at;
  if (diagnostic_frames < 120)
    return;
  const double wall_seconds =
      (double)(finished_at - diagnostic_window_started) / 1000000000.0;
  const double average_ms =
      (double)diagnostic_render_nanoseconds / diagnostic_frames / 1000000.0;
  const double maximum_ms =
      (double)diagnostic_max_render_nanoseconds / 1000000.0;
  const double frames_per_second = diagnostic_frames / wall_seconds;
  printf("InfoNES: Video diagnostics frames=%llu fps=%.2f render_avg_ms=%.3f "
         "render_max_ms=%.3f\n",
         (unsigned long long)diagnostic_frames, frames_per_second, average_ms,
         maximum_ms);
  diagnostic_frames = 0;
  diagnostic_render_nanoseconds = 0;
  diagnostic_max_render_nanoseconds = 0;
  diagnostic_window_started = finished_at;
}

static void wait_for_vsync(void) {
  if (fb_vsync_state == 0 || fb_fd < 0)
    return;
  int argument = 0;
  if (ioctl(fb_fd, FBIO_WAITFORVSYNC, &argument) == 0) {
    if (fb_vsync_state == 1)
      printf("InfoNES: Framebuffer vsync enabled\n");
    fb_vsync_state = 2;
    return;
  }
  if (fb_vsync_state == 1)
    printf("InfoNES: Framebuffer vsync unavailable: %s\n", strerror(errno));
  fb_vsync_state = 0;
}

/*-------------------------------------------------------------------*/
/*  NES Palette - RGB565 format                                      */
/*-------------------------------------------------------------------*/

WORD NesPalette[64] = {
    0x39ce, 0x1071, 0x0015, 0x2013, 0x440e, 0x5402, 0x5000, 0x3c20,
    0x20a0, 0x0100, 0x0140, 0x00e2, 0x0ceb, 0x0000, 0x0000, 0x0000,
    0x5ef7, 0x01dd, 0x10fd, 0x401e, 0x5c17, 0x700b, 0x6ca0, 0x6521,
    0x45c0, 0x0240, 0x02a0, 0x0247, 0x0211, 0x0000, 0x0000, 0x0000,
    0x7fff, 0x1eff, 0x2e5f, 0x223f, 0x79ff, 0x7dd6, 0x7dcc, 0x7e67,
    0x7ae7, 0x4342, 0x2769, 0x2ff3, 0x03bb, 0x0000, 0x0000, 0x0000,
    0x7fff, 0x579f, 0x635f, 0x6b3f, 0x7f1f, 0x7f1b, 0x7ef6, 0x7f75,
    0x7f94, 0x73f4, 0x57d7, 0x5bf9, 0x4ffe, 0x0000, 0x0000, 0x0000};

/*-------------------------------------------------------------------*/
/*  Convert RGB555 palette to framebuffer format using FB offsets    */
/*-------------------------------------------------------------------*/

static inline uint16_t rgb555_to_fb(uint16_t rgb) {
  // Extract from RGB555: XRRRRRGGGGGBBBBB (5-5-5 format)
  uint16_t r = (rgb >> 10) & 0x1F; // 5 bits at offset 10
  uint16_t g = (rgb >> 5) & 0x1F;  // 5 bits at offset 5
  uint16_t b = rgb & 0x1F;         // 5 bits at offset 0

  // Scale to framebuffer bit depths and place at expected offsets
  // FB is likely RGB565: need to scale G from 5 to 6 bits
  uint16_t fb_r = r << (fb_var.red.length - 5);
  uint16_t fb_g = g << (fb_var.green.length - 5);
  uint16_t fb_b = b << (fb_var.blue.length - 5);

  return (fb_r << fb_var.red.offset) | (fb_g << fb_var.green.offset) |
         (fb_b << fb_var.blue.offset);
}

/*-------------------------------------------------------------------*/
/*  Framebuffer initialization                                       */
/*-------------------------------------------------------------------*/

static int lcd_fb_init(void) {
  fb_fd = open("/dev/fb0", O_RDWR);
  if (fb_fd < 0) {
    printf("InfoNES: Cannot open /dev/fb0\n");
    return -1;
  }

  if (ioctl(fb_fd, FBIOGET_VSCREENINFO, &fb_var) < 0) {
    printf("InfoNES: Cannot get framebuffer info\n");
    close(fb_fd);
    fb_fd = -1;
    return -1;
  }

  if (ioctl(fb_fd, FBIOGET_FSCREENINFO, &fb_fix) < 0) {
    printf("InfoNES: Cannot get fixed framebuffer info\n");
    close(fb_fd);
    fb_fd = -1;
    return -1;
  }

  fb_bpp = fb_var.bits_per_pixel / 8;
  fb_stride = fb_fix.line_length;
  fb_map_size = fb_fix.smem_len;

  if ((fb_bpp != 2 && fb_bpp != 4) || fb_stride <= 0 ||
      (size_t)fb_stride < (size_t)fb_var.xres * fb_bpp) {
    printf("InfoNES: Unsupported framebuffer layout\n");
    close(fb_fd);
    fb_fd = -1;
    return -1;
  }

  const unsigned int mapped_rows =
      fb_var.yres_virtual ? fb_var.yres_virtual : fb_var.yres;
  const size_t required_size = (size_t)fb_stride * mapped_rows;
  if (fb_map_size == 0)
    fb_map_size = required_size;
  else if (fb_map_size < required_size) {
    printf("InfoNES: Framebuffer memory is smaller than its advertised layout\n");
    close(fb_fd);
    fb_fd = -1;
    return -1;
  }

  printf("InfoNES: Framebuffer %dx%d, %d bpp, stride %d, map %zu bytes\n",
         fb_var.xres, fb_var.yres, fb_var.bits_per_pixel, fb_stride,
         fb_map_size);
  printf("InfoNES: Color format - R:%d@%d G:%d@%d B:%d@%d\n", fb_var.red.length,
         fb_var.red.offset, fb_var.green.length, fb_var.green.offset,
         fb_var.blue.length, fb_var.blue.offset);

  // Map framebuffer memory
  fb_mem = (unsigned char *)mmap(NULL, fb_map_size, PROT_READ | PROT_WRITE,
                                 MAP_SHARED, fb_fd, 0);
  if (fb_mem == MAP_FAILED) {
    printf("InfoNES: Cannot mmap framebuffer\n");
    fb_mem = NULL;
    close(fb_fd);
    fb_fd = -1;
    return -1;
  }

  // Render directly into the mapping: avoid a second 1.6 MiB frame buffer.
  memset(fb_mem, 0, fb_map_size);

  // Calculate scaling factor
#if FB_ROTATION_90
  fb_scaling = DISPLAY_WIDTH / NES_DISP_WIDTH;
  if (DISPLAY_HEIGHT / NES_DISP_HEIGHT < fb_scaling)
    fb_scaling = DISPLAY_HEIGHT / NES_DISP_HEIGHT;
#else
  fb_scaling = fb_var.xres / NES_DISP_WIDTH;
  if (fb_var.yres / NES_DISP_HEIGHT < fb_scaling)
    fb_scaling = fb_var.yres / NES_DISP_HEIGHT;
#endif

  printf("InfoNES: Display scaling: %dx\n", fb_scaling);
  printf("InfoNES: Rotation: %s\n", FB_ROTATION_90 ? "90 degrees" : "none");

#if FB_ROTATION_90
  if (fb_bpp == 2 && fb_scaling == 2) {
    fb_staging_rows = NES_DISP_WIDTH * fb_scaling;
    fb_staging_columns = NES_DISP_HEIGHT * fb_scaling;
    fb_staging_first_row =
        ((int)fb_var.xres - 1) -
        (((NES_DISP_WIDTH - 1) * fb_scaling) + FB_OFFSET_X);
    const size_t staging_pixels =
        (size_t)fb_staging_rows * fb_staging_columns;
    fb_staging = (uint16_t *)malloc(staging_pixels * sizeof(*fb_staging));
    if (fb_staging)
      printf("InfoNES: Staged video publisher enabled (%zu bytes)\n",
             staging_pixels * sizeof(*fb_staging));
    else
      printf("InfoNES: Cannot allocate staged video frame; using direct "
             "framebuffer writes\n");
  }
#endif
  runtime_diagnostics =
      environment_flag("INFONES_RUNTIME_DIAGNOSTICS", FALSE);
  fb_vsync_state = environment_flag("INFONES_VSYNC", FALSE) ? 1 : 0;
  if (runtime_diagnostics)
    printf("InfoNES: Runtime video diagnostics enabled\n");
  if (fb_vsync_state == 0)
    printf("InfoNES: Framebuffer vsync disabled\n");

  return 0;
}

/*-------------------------------------------------------------------*/
/*  Function prototypes                                              */
/*-------------------------------------------------------------------*/

void *emulation_thread(void *args);
void start_application(char *filename);
int LoadSRAM(void);
int SaveSRAM(void);

/*===================================================================*/
/*                           main()                                  */
/*===================================================================*/

int main(int argc, char **argv) {
  setvbuf(stdout, NULL, _IOLBF, 0);

  struct sigaction shutdown_action;
  memset(&shutdown_action, 0, sizeof(shutdown_action));
  shutdown_action.sa_handler = request_shutdown;
  sigemptyset(&shutdown_action.sa_mask);
  sigaction(SIGINT, &shutdown_action, NULL);
  sigaction(SIGTERM, &shutdown_action, NULL);

  dwKeyPad1 = 0;
  dwKeyPad2 = 0;
  dwKeySystem = 0;
  bThread = FALSE;

  if (InitJoypadInput() < 0)
    printf("InfoNES: Continuing without controller input\n");

  if (lcd_fb_init() < 0) {
    printf("InfoNES: Framebuffer initialization failed\n");
    return -1;
  }

  if (argc == 2) {
    start_application(argv[1]);
    if (!emulation_thread_started)
      return -1;
  } else {
    printf("Usage: %s <rom.nes>\n", argv[0]);
    return -1;
  }

  // Main loop - handle input
  while (!shutdown_requested) {
    dwKeyPad1 = GetJoypadInput(0);
    dwKeyPad2 = GetJoypadInput(1);
    usleep(1000);
  }

  printf("InfoNES: Shutdown requested\n");
  dwKeySystem = PAD_SYS_QUIT;
  bThread = FALSE;
  if (emulation_thread_started) {
    pthread_join(emulation_tid, NULL);
    emulation_thread_started = 0;
    SaveSRAM();
  }

  return 0;
}

/*===================================================================*/
/*                     emulation_thread()                            */
/*===================================================================*/

void *emulation_thread(void *args) {
  InfoNES_Main();
  return NULL;
}

/*===================================================================*/
/*                    start_application()                            */
/*===================================================================*/

void start_application(char *filename) {
  strcpy(szRomName, filename);

  if (InfoNES_Load(szRomName) == 0) {
    LoadSRAM();
    bThread = TRUE;
    if (pthread_create(&emulation_tid, NULL, emulation_thread, NULL) == 0) {
      emulation_thread_started = 1;
    } else {
      bThread = FALSE;
      printf("InfoNES: Failed to start emulation thread\n");
    }
  } else {
    printf("InfoNES: Failed to load ROM: %s\n", filename);
  }
}

/*===================================================================*/
/*                        LoadSRAM()                                 */
/*===================================================================*/

int LoadSRAM(void) {
  FILE *fp;
  unsigned char pSrcBuf[SRAM_SIZE];
  unsigned char chData, chTag;
  int nRunLen, nDecoded, nDecLen, nIdx;

  nSRAM_SaveFlag = 0;

  if (!ROM_SRAM)
    return 0;

  nSRAM_SaveFlag = 1;

  strcpy(szSaveName, szRomName);
  strcpy(strrchr(szSaveName, '.') + 1, "srm");

  fp = fopen(szSaveName, "rb");
  if (fp == NULL)
    return -1;

  fread(pSrcBuf, SRAM_SIZE, 1, fp);
  fclose(fp);

  nDecoded = 0;
  nDecLen = 0;
  chTag = pSrcBuf[nDecoded++];

  while (nDecLen < 8192) {
    chData = pSrcBuf[nDecoded++];

    if (chData == chTag) {
      chData = pSrcBuf[nDecoded++];
      nRunLen = pSrcBuf[nDecoded++];
      for (nIdx = 0; nIdx < nRunLen + 1; ++nIdx) {
        SRAM[nDecLen++] = chData;
      }
    } else {
      SRAM[nDecLen++] = chData;
    }
  }

  return 0;
}

/*===================================================================*/
/*                        SaveSRAM()                                 */
/*===================================================================*/

int SaveSRAM(void) {
  FILE *fp;
  int nUsedTable[256];
  unsigned char chData, chPrevData, chTag;
  int nIdx, nEncoded, nEncLen, nRunLen;
  unsigned char pDstBuf[SRAM_SIZE];

  if (!nSRAM_SaveFlag)
    return 0;

  memset(nUsedTable, 0, sizeof(nUsedTable));

  for (nIdx = 0; nIdx < SRAM_SIZE; ++nIdx) {
    ++nUsedTable[SRAM[nIdx++]];
  }
  for (nIdx = 1, chTag = 0; nIdx < 256; ++nIdx) {
    if (nUsedTable[nIdx] < nUsedTable[chTag])
      chTag = nIdx;
  }

  nEncoded = 0;
  nEncLen = 0;
  nRunLen = 1;

  pDstBuf[nEncLen++] = chTag;
  chPrevData = SRAM[nEncoded++];

  while (nEncoded < SRAM_SIZE && nEncLen < SRAM_SIZE - 133) {
    chData = SRAM[nEncoded++];

    if (chPrevData == chData && nRunLen < 256)
      ++nRunLen;
    else {
      if (nRunLen >= 4 || chPrevData == chTag) {
        pDstBuf[nEncLen++] = chTag;
        pDstBuf[nEncLen++] = chPrevData;
        pDstBuf[nEncLen++] = nRunLen - 1;
      } else {
        for (nIdx = 0; nIdx < nRunLen; ++nIdx)
          pDstBuf[nEncLen++] = chPrevData;
      }
      chPrevData = chData;
      nRunLen = 1;
    }
  }

  if (nRunLen >= 4 || chPrevData == chTag) {
    pDstBuf[nEncLen++] = chTag;
    pDstBuf[nEncLen++] = chPrevData;
    pDstBuf[nEncLen++] = nRunLen - 1;
  } else {
    for (nIdx = 0; nIdx < nRunLen; ++nIdx)
      pDstBuf[nEncLen++] = chPrevData;
  }

  fp = fopen(szSaveName, "wb");
  if (fp == NULL)
    return -1;

  fwrite(pDstBuf, nEncLen, 1, fp);
  fclose(fp);

  return 0;
}

/*===================================================================*/
/*                      InfoNES_Menu()                               */
/*===================================================================*/

int InfoNES_Menu(void) {
  if (bThread == FALSE)
    return -1;
  return 0;
}

/*===================================================================*/
/*                    InfoNES_ReadRom()                              */
/*===================================================================*/

int InfoNES_ReadRom(const char *pszFileName) {
  FILE *fp;

  fp = fopen(pszFileName, "rb");
  if (fp == NULL)
    return -1;

  fread(&NesHeader, sizeof(NesHeader), 1, fp);
  if (memcmp(NesHeader.byID, "NES\x1a", 4) != 0) {
    fclose(fp);
    return -1;
  }

  memset(SRAM, 0, SRAM_SIZE);

  if (NesHeader.byInfo1 & 4) {
    fread(&SRAM[0x1000], 512, 1, fp);
  }

  ROM = (BYTE *)malloc(NesHeader.byRomSize * 0x4000);
  fread(ROM, 0x4000, NesHeader.byRomSize, fp);

  if (NesHeader.byVRomSize > 0) {
    VROM = (BYTE *)malloc(NesHeader.byVRomSize * 0x2000);
    fread(VROM, 0x2000, NesHeader.byVRomSize, fp);
  }

  fclose(fp);
  return 0;
}

/*===================================================================*/
/*                   InfoNES_ReleaseRom()                            */
/*===================================================================*/

void InfoNES_ReleaseRom(void) {
  if (ROM) {
    free(ROM);
    ROM = NULL;
  }
  if (VROM) {
    free(VROM);
    VROM = NULL;
  }
}

/*===================================================================*/
/*                  InfoNES_MemoryCopy()                             */
/*===================================================================*/

void *InfoNES_MemoryCopy(void *dest, const void *src, int count) {
  memcpy(dest, src, count);
  return dest;
}

/*===================================================================*/
/*                   InfoNES_MemorySet()                             */
/*===================================================================*/

void *InfoNES_MemorySet(void *dest, int c, int count) {
  memset(dest, c, count);
  return dest;
}

/*===================================================================*/
/*                   InfoNES_LoadFrame()                             */
/*  Transfer NES frame to rotated framebuffer                        */
/*===================================================================*/

void InfoNES_LoadFrame(void) {
  if (fb_fd < 0 || !fb_mem)
    return;

  const int64_t render_started =
      runtime_diagnostics ? monotonic_nanoseconds() : 0;

  int x, y, sx, sy;
  uint16_t color;
  int bpp = fb_bpp;

#if FB_ROTATION_90
  if (fb_staging) {
    const size_t staging_row_bytes =
        (size_t)fb_staging_columns * sizeof(*fb_staging);
    for (x = 0; x < NES_DISP_WIDTH; x++) {
      const int first_physical_row =
          ((int)fb_var.xres - 1) - (x * fb_scaling + FB_OFFSET_X);
      uint16_t *destination =
          fb_staging +
          (size_t)(first_physical_row - fb_staging_first_row) *
              fb_staging_columns;
      for (y = 0; y < NES_DISP_HEIGHT; y++) {
        color = rgb555_to_fb(WorkFrame[y * NES_DISP_WIDTH + x]);
        destination[y * fb_scaling] = color;
        destination[y * fb_scaling + 1] = color;
      }
      memcpy(destination + fb_staging_columns, destination,
             staging_row_bytes);
    }

    // Build the complete rotated image in cacheable memory first. If optional
    // scanout synchronization is enabled, wait only after staging is complete.
    wait_for_vsync();
    for (int row = 0; row < fb_staging_rows; row++) {
      unsigned char *destination =
          fb_mem + (size_t)(fb_staging_first_row + row) * fb_stride +
          (size_t)FB_OFFSET_Y * fb_bpp;
      memcpy(destination,
             fb_staging + (size_t)row * fb_staging_columns,
             staging_row_bytes);
    }
  } else {
    wait_for_vsync();
    // Compatibility path for framebuffer layouts other than the live Deck's
    // 600x1280 RGB565 surface.
    for (y = 0; y < NES_DISP_HEIGHT; y++) {
      for (x = 0; x < NES_DISP_WIDTH; x++) {
        color = rgb555_to_fb(WorkFrame[y * NES_DISP_WIDTH + x]);
        int offset_x = x * fb_scaling + FB_OFFSET_X;
        int phys_col = ((int)fb_var.xres - 1) - offset_x;
        int phys_row = y * fb_scaling + FB_OFFSET_Y;
        for (sy = 0; sy < fb_scaling; sy++) {
          for (sx = 0; sx < fb_scaling; sx++) {
            int px = phys_row + sy;
            int py = phys_col + sx;
            if (px >= 0 && px < DISPLAY_HEIGHT && px < (int)fb_var.xres &&
                py >= 0 && py < DISPLAY_WIDTH && py < (int)fb_var.yres) {
              unsigned char *pixel = fb_mem + ((size_t)py * fb_stride) +
                                     ((size_t)px * bpp);
              if (bpp == 2)
                *(uint16_t *)pixel = color;
              else if (bpp == 4)
                *(uint32_t *)pixel = color;
            }
          }
        }
      }
    }
  }
#else
  wait_for_vsync();
  // Non-rotated rendering (for reference/testing)
  for (y = 0; y < NES_DISP_HEIGHT; y++) {
    for (x = 0; x < NES_DISP_WIDTH; x++) {
      color = WorkFrame[y * NES_DISP_WIDTH + x];

      // Convert to framebuffer format using actual FB offsets
      color = rgb555_to_fb(color);

      for (sy = 0; sy < fb_scaling; sy++) {
        for (sx = 0; sx < fb_scaling; sx++) {
          int px = x * fb_scaling + sx;
          int py = y * fb_scaling + sy;

          if (px < (int)fb_var.xres && py < (int)fb_var.yres) {
            unsigned char *pixel = fb_mem + ((size_t)py * fb_stride) +
                                   ((size_t)px * bpp);

            if (bpp == 2) {
              *(uint16_t *)pixel = color;
            } else if (bpp == 4) {
              *(uint32_t *)pixel = color;
            }
          }
        }
      }
    }
  }
#endif
  record_render_time(render_started);
}

/*===================================================================*/
/*                   InfoNES_PadState()                              */
/*===================================================================*/

void InfoNES_PadState(DWORD *pdwPad1, DWORD *pdwPad2, DWORD *pdwSystem) {
  *pdwPad1 = dwKeyPad1;
  *pdwPad2 = dwKeyPad2;
  *pdwSystem = dwKeySystem;
}

/*===================================================================*/
/*                Sound functions (OSS /dev/dsp)                     */
/*===================================================================*/

static int sound_fd = -1;
static int16_t *sound_mix_buf = NULL;
static int16_t *sound_resample_buf = NULL;
static size_t sound_mix_capacity = 0;
static size_t sound_resample_capacity = 0;
static unsigned int sound_source_rate = 0;
static unsigned int sound_device_rate = 0;
static uint64_t sound_rate_remainder = 0;
static int sound_trigger_pending = 0;
static NesAudioMixer sound_mixer;

static int sound_write_all(const void *buffer, size_t bytes) {
  const unsigned char *data =
      reinterpret_cast<const unsigned char *>(buffer);
  size_t written = 0;
  while (written < bytes) {
    const ssize_t result = write(sound_fd, data + written, bytes - written);
    if (result > 0)
      written += (size_t)result;
    else if (result < 0 && errno == EINTR)
      continue;
    else {
      if (result == 0)
        errno = EIO;
      return 0;
    }
  }
  return 1;
}

void InfoNES_SoundInit(void) {}

int InfoNES_SoundOpen(int samples_per_sync, int sample_rate) {
  int format = AFMT_S16_LE;
  int channels = 1;
  int rate = sample_rate;
  // Eight 1024-byte periods: about 93 ms at 44.1 kHz, mono S16.
  int frag = (8 << 16) | 10;

  if (samples_per_sync <= 0 || sample_rate <= 0)
    return 0;

  sound_fd = open("/dev/dsp", O_WRONLY);
  if (sound_fd < 0) {
    printf("InfoNES: Cannot open /dev/dsp - sound disabled\n");
    return 0;
  }

  // Blocking writes pace emulation.  Two-plus frame callbacks of buffering
  // absorb framebuffer scheduling jitter without adding excessive latency.
  if (ioctl(sound_fd, SNDCTL_DSP_SETFRAGMENT, &frag) < 0)
    printf("InfoNES: OSS fragment request failed: %s\n", strerror(errno));

  // Preserve the mixer's native 16-bit precision all the way into ALSA.
  if (ioctl(sound_fd, SNDCTL_DSP_SETFMT, &format) < 0 ||
      format != AFMT_S16_LE) {
    printf("InfoNES: Cannot set signed 16-bit audio format\n");
    close(sound_fd);
    sound_fd = -1;
    return 0;
  }

  // Set mono
  if (ioctl(sound_fd, SNDCTL_DSP_CHANNELS, &channels) < 0 || channels != 1) {
    printf("InfoNES: Cannot set mono\n");
    close(sound_fd);
    sound_fd = -1;
    return 0;
  }

  // Set sample rate
  if (ioctl(sound_fd, SNDCTL_DSP_SPEED, &rate) < 0) {
    printf("InfoNES: Cannot set sample rate\n");
    close(sound_fd);
    sound_fd = -1;
    return 0;
  }

  if (rate <= 0) {
    printf("InfoNES: OSS returned an invalid sample rate\n");
    close(sound_fd);
    sound_fd = -1;
    return 0;
  }

  /* Hold playback while the ring is primed; enable it on first callback. */
  int trigger = 0;
  if (ioctl(sound_fd, SNDCTL_DSP_SETTRIGGER, &trigger) == 0)
    sound_trigger_pending = 1;
  else
    printf("InfoNES: OSS deferred trigger unavailable: %s\n", strerror(errno));

  sound_mix_capacity = (size_t)samples_per_sync;
  sound_source_rate = (unsigned int)sample_rate;
  sound_device_rate = (unsigned int)rate;
  sound_rate_remainder = 0;
  NesAudioMixer_Reset(&sound_mixer);

  unsigned int volume_percent = 42;
  const char *volume_text = getenv("INFONES_VOLUME_PERCENT");
  if (volume_text && *volume_text) {
    char *end = NULL;
    errno = 0;
    const long parsed = strtol(volume_text, &end, 10);
    if (!errno && end && *end == '\0' && parsed >= 0 && parsed <= 100)
      volume_percent = (unsigned int)parsed;
    else
      printf("InfoNES: Ignoring invalid INFONES_VOLUME_PERCENT=%s\n",
             volume_text);
  }
  NesAudioMixer_SetVolumePercent(&sound_mixer, volume_percent);

  sound_mix_buf = (int16_t *)malloc(sound_mix_capacity * sizeof(*sound_mix_buf));
  if (!sound_mix_buf) {
    printf("InfoNES: Cannot allocate audio mix buffer\n");
    close(sound_fd);
    sound_fd = -1;
    return 0;
  }

  if (sound_device_rate != sound_source_rate) {
    sound_resample_capacity = NesAudio_ResampledCapacity(
        sound_mix_capacity, sound_source_rate, sound_device_rate);
    sound_resample_buf =
        (int16_t *)malloc(sound_resample_capacity * sizeof(*sound_resample_buf));
    if (!sound_resample_buf) {
      printf("InfoNES: Cannot allocate audio resampling buffer\n");
      InfoNES_SoundClose();
      return 0;
    }
  }

  int block_size = 0;
  audio_buf_info space;
  memset(&space, 0, sizeof(space));
  ioctl(sound_fd, SNDCTL_DSP_GETBLKSIZE, &block_size);
  ioctl(sound_fd, SNDCTL_DSP_GETOSPACE, &space);

  /*
   * Preload the complete ring with digital silence while the OSS trigger is
   * paused so framebuffer initialization jitter cannot cause a one-off XRUN
   * before the first emulated frame is ready.
   */
  if (space.bytes > 0 && space.bytes <= 1024 * 1024) {
    void *silence = calloc(1, (size_t)space.bytes);
    if (!silence || !sound_write_all(silence, (size_t)space.bytes)) {
      free(silence);
      printf("InfoNES: Cannot prefill the OSS audio ring\n");
      InfoNES_SoundClose();
      return 0;
    }
    free(silence);
  }

  printf("InfoNES: OSS S16 mono sound opened - %d Hz (requested %d), "
         "%d samples, block %d, buffer %d bytes, volume %u%%\n",
         rate, sample_rate, samples_per_sync, block_size, space.bytes,
         volume_percent);
  return 1;
}

void InfoNES_SoundClose(void) {
  if (sound_fd >= 0) {
    close(sound_fd);
    sound_fd = -1;
  }
  if (sound_mix_buf) {
    free(sound_mix_buf);
    sound_mix_buf = NULL;
  }
  if (sound_resample_buf) {
    free(sound_resample_buf);
    sound_resample_buf = NULL;
  }
  sound_mix_capacity = 0;
  sound_resample_capacity = 0;
  sound_source_rate = 0;
  sound_device_rate = 0;
  sound_rate_remainder = 0;
  sound_trigger_pending = 0;
}

void InfoNES_SoundOutput(int samples, BYTE *wave1, BYTE *wave2, BYTE *wave3,
                         BYTE *wave4, BYTE *wave5) {
  if (sound_fd < 0 || !sound_mix_buf || samples <= 0)
    return;

  if ((size_t)samples > sound_mix_capacity) {
    printf("InfoNES: Dropping oversized audio callback (%d samples)\n", samples);
    return;
  }

  for (int i = 0; i < samples; i++) {
    sound_mix_buf[i] = NesAudioMixer_MixSampleS16(
        &sound_mixer, wave1[i], wave2[i], wave3[i], wave4[i] & 0x0f,
        wave5[i] & 0x7f);
  }

  const int16_t *output = sound_mix_buf;
  size_t output_samples = (size_t)samples;
  if (sound_device_rate != sound_source_rate) {
    const uint64_t scaled =
        (uint64_t)samples * sound_device_rate + sound_rate_remainder;
    output_samples = (size_t)(scaled / sound_source_rate);
    sound_rate_remainder = scaled % sound_source_rate;
    if (output_samples > sound_resample_capacity) {
      printf("InfoNES: Dropping oversized resampled audio callback\n");
      return;
    }
    NesAudio_ResampleS16(sound_mix_buf, (size_t)samples, sound_resample_buf,
                         output_samples);
    output = sound_resample_buf;
  }

  const size_t output_size = output_samples * sizeof(*output);
  if (sound_trigger_pending) {
    int trigger = PCM_ENABLE_OUTPUT;
    if (ioctl(sound_fd, SNDCTL_DSP_SETTRIGGER, &trigger) < 0)
      printf("InfoNES: Cannot start deferred OSS playback: %s\n",
             strerror(errno));
    sound_trigger_pending = 0;
  }
  if (!sound_write_all(output, output_size)) {
    printf("InfoNES: Audio write failed: %s; sound disabled\n",
           strerror(errno));
    close(sound_fd);
    sound_fd = -1;
  }
}

/*===================================================================*/
/*                    InfoNES_Wait()                                 */
/*===================================================================*/

void InfoNES_Wait(void) {}

/*===================================================================*/
/*                  InfoNES_MessageBox()                             */
/*===================================================================*/

void InfoNES_MessageBox(const char *pszMsg, ...) {
  printf("InfoNES: %s\n", pszMsg);
}

/*
 * End of InfoNES_System_Deck.cpp
 */
