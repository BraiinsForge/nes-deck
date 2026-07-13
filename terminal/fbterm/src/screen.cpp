/*
 *   Copyright © 2008-2010 dragchan <zgchan317@gmail.com>
 *   This file is part of FbTerm.
 *
 *   This program is free software; you can redistribute it and/or
 *   modify it under the terms of the GNU General Public License
 *   as published by the Free Software Foundation; either version 2
 *   of the License, or (at your option) any later version.
 *
 *   This program is distributed in the hope that it will be useful,
 *   but WITHOUT ANY WARRANTY; without even the implied warranty of
 *   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *   GNU General Public License for more details.
 *
 *   You should have received a copy of the GNU General Public License
 *   along with this program; if not, write to the Free Software
 *   Foundation, Inc., 675 Mass Ave, Cambridge, MA 02139, USA.
 *
 */

#include <stdio.h>
#include <unistd.h>
#include <string.h>
#include "screen.h"
#include "font.h"
#include "fbshellman.h"
#include "fbconfig.h"
#include "fbdev.h"
#include "config.h"
#ifdef ENABLE_VESA
#include "vesadev.h"
#endif

#define MIN(a,b) ((a) < (b) ? (a) : (b))
#define MAX(a,b) ((a) > (b) ? (a) : (b))
#define redraw(args...) (FbShellManager::instance()->redraw(args))

static const s8 show_cursor[] = "\e[?25h";
static const s8 hide_cursor[] = "\e[?25l";
static const s8 disable_blank[] = "\e[9;0]";
static const s8 enable_blank[] = "\e[9;10]";
static const s8 clear_screen[] = "\e[2J\e[H";

DEFINE_INSTANCE(Screen)

Screen *Screen::createInstance()
{
	if (!Font::instance() || !FW(1) || !FH(1)) {
		fprintf(stderr, "init font error!\n");
		return 0;
	}

	Screen *pScreen = 0;

#ifdef ENABLE_VESA
	s8 buf[16];
	Config::instance()->getOption("vesa-mode", buf, sizeof(buf));
	if (!strcmp(buf, "list")) {
		VesaDev::printModes();
		return 0;
	}

	u32 mode = 0;
	Config::instance()->getOption("vesa-mode", mode);

	if (!mode) pScreen = FbDev::initFbDev();
	if (!pScreen) pScreen = VesaDev::initVesaDev(mode);
#else
	pScreen = FbDev::initFbDev();
#endif

	if (!pScreen) return 0;

	if (pScreen->mRotateType == Rotate90 || pScreen->mRotateType == Rotate270) {
		u32 tmp = pScreen->mWidth;
		pScreen->mWidth = pScreen->mHeight;
		pScreen->mHeight = tmp;
	}

	if (!pScreen->mCols) pScreen->mCols = pScreen->mWidth / FW(1);
	if (!pScreen->mRows) pScreen->mRows = pScreen->mHeight / FH(1);

	if (!pScreen->mCols || !pScreen->mRows) {
		fprintf(stderr, "font size is too huge!\n");
		delete pScreen;
		return 0;
	}

	pScreen->initFillDraw();
	return pScreen;
}

Screen::Screen()
{
	mWidth = mHeight = 0;
	mCols = mRows = 0;
	mBitsPerPixel = mBytesPerLine = 0;

	mScrollEnable = true;
	mScrollType = Redraw;
	mOffsetMax = 0;
	mOffsetCur = 0;

	mVMemBase = 0;
	mPalette = 0;

	// Hardcoded for Braiins Forge Deck (equivalent to -r 3)
	mRotateType = Rotate270;

	s32 ret = write(STDIN_FILENO, hide_cursor, sizeof(hide_cursor) - 1);
	ret = write(STDIN_FILENO, disable_blank, sizeof(disable_blank) - 1);
}

Screen::~Screen()
{
	Font::uninstance();
	endFillDraw();

	s32 ret = write(STDIN_FILENO, show_cursor, sizeof(show_cursor) - 1);
	ret = write(STDIN_FILENO, enable_blank, sizeof(enable_blank) - 1);
	ret = write(STDIN_FILENO, clear_screen, sizeof(clear_screen) - 1);
}

void Screen::showInfo(bool verbose)
{
	if (!verbose) return;

	static const s8* const scrollstr[4] = {
		"redraw", "ypan", "ywrap", "xpan"
	};
	printf("[screen] driver: %s, mode: %dx%d-%dbpp, scrolling: %s\n",
		drvId(), mWidth, mHeight, mBitsPerPixel, scrollstr[mScrollType]);
}

void Screen::switchVc(bool enter)
{
	mOffsetCur = 0;
	setupOffset();

	setupPalette(!enter);
	if (enter && mPalette) eraseMargin(true, mRows);
}

bool Screen::move(u16 scol, u16 srow, u16 dcol, u16 drow, u16 w, u16 h)
{
	if (!mScrollEnable || mScrollType == Redraw || scol != dcol) return false;

	u16 top = MIN(srow, drow), bot = MAX(srow, drow) + h;
	u16 left = scol, right = scol + w;

	u32 noaccel_redraw_area = w * (bot - top - 1);
	u32 accel_redraw_area = mCols * mRows - w * h;

	if (noaccel_redraw_area <= accel_redraw_area) return false;

	if (mRotateType == Rotate0 || mRotateType == Rotate270) mOffsetCur += FH((s32)srow - drow);
	else mOffsetCur -= FH((s32)srow - drow);

	bool redraw_all = false;
	if (mScrollType == YPan || mScrollType == XPan) {
		redraw_all = true;

		if (mOffsetCur < 0) mOffsetCur = mOffsetMax;
		else if ((u32)mOffsetCur > mOffsetMax) mOffsetCur = 0;
		else redraw_all = false;
	} else {
		if (mOffsetCur < 0) mOffsetCur += mOffsetMax + 1;
		else if ((u32)mOffsetCur > mOffsetMax) mOffsetCur -= mOffsetMax + 1;
	}

	setupOffset();

	if (top) redraw(0, 0, mCols, top);
	if (bot < mRows) redraw(0, bot, mCols, mRows - bot);
	if (left > 0) redraw(0, top, left, bot - top - 1);
	if (right < mCols) redraw(right, top, mCols - right, bot - top - 1);

	if (redraw_all) {
		eraseMargin(true, mRows);
	} else {
		eraseMargin(drow > srow, drow > srow ? (drow - srow) : (srow - drow));
	}

	return !redraw_all;
}

void Screen::eraseMargin(bool top, u16 h)
{
	if (mWidth % FW(1)) {
		fillRect(FW(mCols), top ? 0 : FH(mRows - h), mWidth % FW(1), FH(h), 0);
	}

	if (mHeight % FH(1)) {
		fillRect(0, FH(mRows), mWidth, mHeight % FH(1), 0);
	}
}

void Screen::drawText(u32 x, u32 y, u8 fc, u8 bc, u16 num, u16 *text, bool *dw)
{
	u32 startx, fw = FW(1);

	u16 startnum, *starttext;
	bool *startdw, draw_space = false, draw_text = false;

	for (; num; num--, text++, dw++, x += fw) {
		if (*text == 0x20) {
			if (draw_text) {
				draw_text = false;
				drawGlyphs(startx, y, fc, bc, startnum - num, starttext, startdw);
			}

			if (!draw_space) {
				draw_space = true;
				startx = x;
			}
		} else {
			if (draw_space) {
				draw_space = false;
				fillRect(startx, y, x - startx, FH(1), bc);
			}

			if (!draw_text) {
				draw_text = true;
				starttext = text;
				startdw = dw;
				startnum = num;
				startx = x;
			}

			if (*dw) x += fw;
		}
	}

	if (draw_text) {
		drawGlyphs(startx, y, fc, bc, startnum - num, starttext, startdw);
	} else if (draw_space) {
		fillRect(startx, y, x - startx, FH(1), bc);
	}
}

void Screen::drawGlyphs(u32 x, u32 y, u8 fc, u8 bc, u16 num, u16 *text, bool *dw)
{
	for (; num--; text++, dw++) {
		drawGlyph(x, y, fc, bc, *text, *dw);
		x += *dw ? FW(2) : FW(1);
	}
}

void Screen::adjustOffset(u32 &x, u32 &y)
{
	if (mScrollType == XPan) x += mOffsetCur;
	else y += mOffsetCur;
}

void Screen::fillRect(u32 x, u32 y, u32 w, u32 h, u8 color)
{
	if (x >= mWidth || y >= mHeight || !w || !h) return;
	if (x + w > mWidth) w = mWidth - x;
	if (y + h > mHeight) h = mHeight - y;

	rotateRect(x, y, w, h);
	adjustOffset(x, y);

	for (; h--;) {
		if (mScrollType == YWrap && y > mOffsetMax) y -= mOffsetMax + 1;
		(this->*fill)(x, y++, w, color);
	}
}

void Screen::drawGlyph(u32 x, u32 y, u8 fc, u8 bc, u16 code, bool dw)
{
	if (x >= mWidth || y >= mHeight) return;

	s32 w = (dw ? FW(2) : FW(1)), h = FH(1);
	if (x + w > mWidth) w = mWidth - x;
	if (y + h > mHeight) h = mHeight - y;

	Font::Glyph *glyph = (Font::Glyph *)Font::instance()->getGlyph(code);
	if (!glyph) {
		fillRect(x, y, w, h, bc);
		return;
	}

	/* Clear the complete cell first, then clip the glyph and its source bitmap
	 * independently.  The old bearing arithmetic could underflow for a glyph
	 * with a negative left bearing and read unrelated pixels as glyph data. */
	fillRect(x, y, w, h, bc);
	s32 dest_left = glyph->left;
	s32 dest_top = glyph->top;
	s32 source_x = 0;
	s32 source_y = 0;
	if (dest_left < 0) source_x = -dest_left, dest_left = 0;
	if (dest_top < 0) source_y = -dest_top, dest_top = 0;
	s32 width = glyph->width - source_x;
	s32 height = glyph->height - source_y;
	if (width > w - dest_left) width = w - dest_left;
	if (height > h - dest_top) height = h - dest_top;
	if ((s32)x + dest_left + width > (s32)mWidth)
		width = mWidth - ((s32)x + dest_left);
	if ((s32)y + dest_top + height > (s32)mHeight)
		height = mHeight - ((s32)y + dest_top);
	if (width <= 0 || height <= 0) return;

	x += dest_left;
	y += dest_top;
	if (x >= mWidth || y >= mHeight || !width || !height) return;

	u32 nwidth = width, nheight = height;
	rotateRect(x, y, nwidth, nheight);

	u8 *pixmap = glyph->pixmap;
	s32 physical_x = 0, physical_y = 0;
	switch (mRotateType) {
	case Rotate0:
		physical_x = source_x;
		physical_y = source_y;
		break;
	case Rotate90:
		physical_x = glyph->height - source_y - height;
		physical_y = source_x;
		break;
	case Rotate180:
		physical_x = glyph->width - source_x - width;
		physical_y = glyph->height - source_y - height;
		break;
	case Rotate270:
		physical_x = source_y;
		physical_y = glyph->width - source_x - width;
		break;
	}
	pixmap += physical_y * glyph->pitch + physical_x;

	adjustOffset(x, y);
	for (; nheight--; y++, pixmap += glyph->pitch) {
		if ((mScrollType == YWrap) && y > mOffsetMax) y -= mOffsetMax + 1;
		(this->*draw)(x, y, nwidth, fc, bc, pixmap);
	}
}

void Screen::rotateRect(u32 &x, u32 &y, u32 &w, u32 &h)
{
	u32 tmp;
	switch (mRotateType) {
	case Rotate0:
		break;

	case Rotate90:
		tmp = x;
		x = mHeight - y - h;
		y = tmp;

		tmp = w;
		w = h;
		h = tmp;
		break;

	case Rotate180:
		x = mWidth - x - w;
		y = mHeight - y - h;
		break;

	case Rotate270:
		tmp = y;
		y = mWidth - x - w;
		x = tmp;

		tmp = w;
		w = h;
		h = tmp;
		break;
	}
}

void Screen::rotatePoint(u32 W, u32 H, u32 &x, u32 &y)
{
	u32 tmp;
	switch (mRotateType) {
	case Rotate0:
		break;

	case Rotate90:
		tmp = x;
		x = H - y - 1;
		y = tmp;
		break;

	case Rotate180:
		x = W - x - 1;
		y = H - y - 1;
		break;

	case Rotate270:
		tmp = y;
		y = W - x - 1;
		x = tmp;
		break;
	}
}
