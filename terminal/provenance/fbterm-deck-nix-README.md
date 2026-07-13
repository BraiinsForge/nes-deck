# Framebuffer Terminal for Braiins Forge Deck

Run a full-featured terminal emulator directly on your Braiins Forge Deck's framebuffer! This project provides statically compiled framebuffer terminals optimized for the Deck's STM32MP1 display hardware.

## What You Need

- **Braiins Forge Deck**
- **USB-C PD Power Adapter**
- **USB-C Hub with PD Support**
- **USB Keyboard**
- **TrueType Font** (ProFont and Minecraftia font are included and recommended -- Minecraftia render's the best)

## Quick Start

### 1. Download Pre-compiled Files

Download the latest release from the [releases page](https://github.com/BraiinsForge/deck-fbterm/releases/latest).

**Available Binaries:**
- `fbterm` - Full-featured terminal with fontconfig, screen rotation, and custom fonts

The release also includes `Minecraftia-Regular.ttf`, the recommended font for best rendering quality.

Also Minecraft mining bitcoin mining?

### 2. Access Your Deck via SSH

```bash
ssh root@<deck-ip>  # Use the admin password you set during setup
```

### 3. Stop the Deck Application

```bash
service bmc stop
```

### 4. Copy Files to Your Deck

```bash
scp fbterm Minecraftia-Regular.ttf root@<deck-ip>:/tmp/
ssh root@<deck-ip> "cp /tmp/Minecraftia-Regular.ttf /usr/share/fonts/"
```

### 5. Run the Terminal

```bash
ssh root@<deck-ip>
chmod +x /tmp/fbterm
/tmp/fbterm -r 3 -n "Minecraftia" -s 16 < /dev/tty1 > /dev/tty1 2>&1
```

## Recommended Setup

The **Minecraftia** font renders best on the Deck's display:

```bash
./fbterm -r 3 -n "Minecraftia" -s 16 < /dev/tty1 > /dev/tty1 2>&1
```

## Command Line Options

### fbterm

| Option | Description |
|--------|-------------|
| `-r 3` | Rotate screen 270° (landscape mode for Deck) |
| `-n "FontName"` | Set font family name |
| `-s 14` | Set font pixel size |
| `-W 11` | Force font width |
| `-H 16` | Force font height |
| `-B 12` | Force font baseline |
| `-a` | Enable anti-aliasing |
| `-f 7` | Foreground color (0-7) |
| `-b 0` | Background color (0-7) |

## Example Commands

**Recommended (Minecraftia font):**
```bash
./fbterm -n "Minecraftia" -s 16 < /dev/tty1 > /dev/tty1 2>&1
```

**Run a specific command (e.g., htop):**
```bash
./fbterm -n "Minecraftia" -s 16 -- htop < /dev/tty1 > /dev/tty1 2>&1
```

This is a good way to run the terminal:

```bash
./bin/fbterm -n "Minecraftia" -s 16 --font-width 14 -a -- sh < /dev/tty1 > /dev/tty1 2>&1
```

## Building from Source

Want to compile the terminal yourself? Check out [BUILD.md](BUILD.md) for complete instructions.

## License

fbterm is licensed under the GNU GPL v2. See the source repository for details.
