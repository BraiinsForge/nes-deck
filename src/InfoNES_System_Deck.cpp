/*===================================================================*/
/*                                                                   */
/*  InfoNES_System_Deck.cpp : Braiins Forge Deck framebuffer port    */
/*                                                                   */
/*  Based on InfoNES_System_Linux.cpp                                */
/*  Modified for STM32MP1 landscape display with 90-degree rotation  */
/*                                                                   */
/*===================================================================*/

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <stdint.h>

#include <sys/types.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <sys/ioctl.h>
#include <unistd.h>
#include <linux/fb.h>
#include <sys/mman.h>
#include <termios.h>

#include "../InfoNES.h"
#include "../InfoNES_System.h"
#include "../InfoNES_pAPU.h"

#define TRUE 1
#define FALSE 0

/*-------------------------------------------------------------------*/
/*  Braiins Forge Deck display configuration                         */
/*-------------------------------------------------------------------*/

// STM32MP1 display configuration (same as fbDOOM)
#define FB_ROTATION_90      1       // Portrait LCD used in landscape mode
#define FB_LINE_OFFSET      120     // First 120 lines are invisible
#define FB_OFFSET_X         -320    // Horizontal offset
#define FB_OFFSET_Y         40      // Vertical offset

// Physical framebuffer dimensions
#define FB_PHYS_WIDTH       640
#define FB_PHYS_HEIGHT      1280

// Effective display dimensions (landscape)
#define DISPLAY_WIDTH       1280
#define DISPLAY_HEIGHT      480

/*-------------------------------------------------------------------*/
/*  Framebuffer variables                                            */
/*-------------------------------------------------------------------*/

static int fb_fd = -1;
static unsigned char *fb_mem = NULL;
static unsigned char *frame_buffer = NULL;
static struct fb_var_screeninfo fb_var;
static int fb_bpp;
static int fb_stride;
static int fb_scaling = 1;

extern "C" int InitJoypadInput(void);
extern "C" int GetJoypadInput(void);

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

DWORD dwKeyPad1;
DWORD dwKeyPad2;
DWORD dwKeySystem;

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
    0x7f94, 0x73f4, 0x57d7, 0x5bf9, 0x4ffe, 0x0000, 0x0000, 0x0000
};

/*-------------------------------------------------------------------*/
/*  Convert RGB565 to BGR565 for STM32MP1 DRM                        */
/*-------------------------------------------------------------------*/

static inline uint16_t rgb565_to_bgr565(uint16_t rgb)
{
    // RGB565: RRRRRGGGGGGBBBBB
    // BGR565: BBBBBGGGGGGRRRRR
    uint16_t r = (rgb >> 11) & 0x1F;
    uint16_t g = (rgb >> 5) & 0x3F;
    uint16_t b = rgb & 0x1F;
    return (b << 11) | (g << 5) | r;
}

/*-------------------------------------------------------------------*/
/*  Framebuffer initialization                                       */
/*-------------------------------------------------------------------*/

static int lcd_fb_init(void)
{
    fb_fd = open("/dev/fb0", O_RDWR);
    if (fb_fd < 0) {
        printf("InfoNES: Cannot open /dev/fb0\n");
        return -1;
    }

    if (ioctl(fb_fd, FBIOGET_VSCREENINFO, &fb_var) < 0) {
        printf("InfoNES: Cannot get framebuffer info\n");
        close(fb_fd);
        return -1;
    }

    fb_bpp = fb_var.bits_per_pixel / 8;
    fb_stride = FB_PHYS_WIDTH * fb_bpp;

    printf("InfoNES: Framebuffer %dx%d, %d bpp\n",
           fb_var.xres, fb_var.yres, fb_var.bits_per_pixel);
    printf("InfoNES: Color format - R:%d@%d G:%d@%d B:%d@%d\n",
           fb_var.red.length, fb_var.red.offset,
           fb_var.green.length, fb_var.green.offset,
           fb_var.blue.length, fb_var.blue.offset);

    // Map framebuffer memory
    size_t fb_size = FB_PHYS_WIDTH * FB_PHYS_HEIGHT * fb_bpp;
    fb_mem = (unsigned char *)mmap(NULL, fb_size,
                                    PROT_READ | PROT_WRITE, MAP_SHARED,
                                    fb_fd, 0);
    if (fb_mem == MAP_FAILED) {
        printf("InfoNES: Cannot mmap framebuffer\n");
        close(fb_fd);
        return -1;
    }

    // Allocate working frame buffer
    frame_buffer = (unsigned char *)malloc(fb_size);
    if (!frame_buffer) {
        printf("InfoNES: Cannot allocate frame buffer\n");
        munmap(fb_mem, fb_size);
        close(fb_fd);
        return -1;
    }

    // Clear screen
    memset(fb_mem, 0, fb_size);
    memset(frame_buffer, 0, fb_size);

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

int main(int argc, char **argv)
{
    dwKeyPad1 = 0;
    dwKeyPad2 = 0;
    dwKeySystem = 0;
    bThread = FALSE;

    InitJoypadInput();

    if (lcd_fb_init() < 0) {
        printf("InfoNES: Framebuffer initialization failed\n");
        return -1;
    }

    if (argc == 2) {
        start_application(argv[1]);
    } else {
        printf("Usage: %s <rom.nes>\n", argv[0]);
        return -1;
    }

    // Main loop - handle input
    while (1) {
        dwKeyPad1 = GetJoypadInput();
        usleep(1000);
    }

    return 0;
}

/*===================================================================*/
/*                     emulation_thread()                            */
/*===================================================================*/

void *emulation_thread(void *args)
{
    InfoNES_Main();
    return NULL;
}

/*===================================================================*/
/*                    start_application()                            */
/*===================================================================*/

void start_application(char *filename)
{
    strcpy(szRomName, filename);

    if (InfoNES_Load(szRomName) == 0) {
        LoadSRAM();
        bThread = TRUE;
        pthread_create(&emulation_tid, NULL, emulation_thread, NULL);
    } else {
        printf("InfoNES: Failed to load ROM: %s\n", filename);
    }
}

/*===================================================================*/
/*                        LoadSRAM()                                 */
/*===================================================================*/

int LoadSRAM(void)
{
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

int SaveSRAM(void)
{
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

int InfoNES_Menu(void)
{
    if (bThread == FALSE)
        return -1;
    return 0;
}

/*===================================================================*/
/*                    InfoNES_ReadRom()                              */
/*===================================================================*/

int InfoNES_ReadRom(const char *pszFileName)
{
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

void InfoNES_ReleaseRom(void)
{
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

void *InfoNES_MemoryCopy(void *dest, const void *src, int count)
{
    memcpy(dest, src, count);
    return dest;
}

/*===================================================================*/
/*                   InfoNES_MemorySet()                             */
/*===================================================================*/

void *InfoNES_MemorySet(void *dest, int c, int count)
{
    memset(dest, c, count);
    return dest;
}

/*===================================================================*/
/*                   InfoNES_LoadFrame()                             */
/*  Transfer NES frame to STM32MP1 rotated framebuffer               */
/*===================================================================*/

void InfoNES_LoadFrame(void)
{
    if (fb_fd < 0 || !fb_mem || !frame_buffer)
        return;

    int x, y, sx, sy;
    uint16_t color;
    int bpp = fb_bpp;

    // Clear frame buffer
    memset(frame_buffer, 0, FB_PHYS_WIDTH * FB_PHYS_HEIGHT * bpp);

#if FB_ROTATION_90
    // Rotated rendering for landscape-oriented portrait display
    // Same transformation as fbDOOM
    for (y = 0; y < NES_DISP_HEIGHT; y++) {
        for (x = 0; x < NES_DISP_WIDTH; x++) {
            // Get NES pixel color (already in RGB565 from WorkFrame)
            color = WorkFrame[y * NES_DISP_WIDTH + x];

            // Convert RGB565 to BGR565 for STM32MP1 DRM
            if (fb_var.blue.offset > fb_var.red.offset) {
                color = rgb565_to_bgr565(color);
            }

            // Apply transformation: offset, mirror X, map to rotated display
            // Same coordinate transformation as fbDOOM
            for (sy = 0; sy < fb_scaling; sy++) {
                for (sx = 0; sx < fb_scaling; sx++) {
                    int offset_x = x * fb_scaling + FB_OFFSET_X;
                    int phys_col = (FB_PHYS_WIDTH - 1) - offset_x - sx;
                    int phys_row = y * fb_scaling + FB_OFFSET_Y + sy;

                    // Bounds check against display dimensions
                    if (phys_row >= 0 && phys_row < DISPLAY_HEIGHT &&
                        phys_col >= 0 && phys_col < DISPLAY_WIDTH) {

                        // Write pixel to frame buffer
                        // Row = phys_col (in rotated space), Col = phys_row
                        unsigned char *pixel = frame_buffer +
                                               (phys_col * fb_stride) +
                                               (phys_row * bpp);

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
#else
    // Non-rotated rendering (for reference/testing)
    for (y = 0; y < NES_DISP_HEIGHT; y++) {
        for (x = 0; x < NES_DISP_WIDTH; x++) {
            color = WorkFrame[y * NES_DISP_WIDTH + x];

            if (fb_var.blue.offset > fb_var.red.offset) {
                color = rgb565_to_bgr565(color);
            }

            for (sy = 0; sy < fb_scaling; sy++) {
                for (sx = 0; sx < fb_scaling; sx++) {
                    int px = x * fb_scaling + sx;
                    int py = y * fb_scaling + sy;

                    if (px < (int)fb_var.xres && py < (int)fb_var.yres) {
                        unsigned char *pixel = frame_buffer +
                                               (py * fb_var.xres * bpp) +
                                               (px * bpp);

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

    // Write to framebuffer
    lseek(fb_fd, 0, SEEK_SET);
    write(fb_fd, frame_buffer, FB_PHYS_WIDTH * FB_PHYS_HEIGHT * bpp);
}

/*===================================================================*/
/*                   InfoNES_PadState()                              */
/*===================================================================*/

void InfoNES_PadState(DWORD *pdwPad1, DWORD *pdwPad2, DWORD *pdwSystem)
{
    *pdwPad1 = dwKeyPad1;
    *pdwPad2 = dwKeyPad2;
    *pdwSystem = dwKeySystem;
}

/*===================================================================*/
/*                Sound functions (stubs - no ALSA)                  */
/*===================================================================*/

void InfoNES_SoundInit(void)
{
}

int InfoNES_SoundOpen(int samples_per_sync, int sample_rate)
{
    // Sound disabled for static build
    return 0;
}

void InfoNES_SoundClose(void)
{
}

void InfoNES_SoundOutput(int samples, BYTE *wave1, BYTE *wave2,
                          BYTE *wave3, BYTE *wave4, BYTE *wave5)
{
    // Sound disabled
}

/*===================================================================*/
/*                    InfoNES_Wait()                                 */
/*===================================================================*/

void InfoNES_Wait(void)
{
}

/*===================================================================*/
/*                  InfoNES_MessageBox()                             */
/*===================================================================*/

void InfoNES_MessageBox(const char *pszMsg, ...)
{
    printf("InfoNES: %s\n", pszMsg);
}

/*
 * End of InfoNES_System_Deck.cpp
 */
