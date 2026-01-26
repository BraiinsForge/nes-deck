# NES Emulator for Braiins Forge Deck

Play classic NES games on your Braiins Forge Deck! This project is based on [InfoNES](https://github.com/nejidev/arm-NES-linux) with custom patches for the Deck's display hardware.

## What You Need

- **Braiins Forge Deck**
- **USB-C PD Power Adapter**
- **USB-C Hub with PD Support**
- **USB Keyboard**
- **NES ROM files** (.nes format)

## Quick Start

### 1. Download Pre-compiled Binary

Download the latest release zip from the [releases page](https://github.com/BraiinsForge/deck-infones/releases/latest).

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
unzip deck-infones.zip
scp infones root@<deck-ip>:/tmp/
scp your-game.nes root@<deck-ip>:/tmp/
```

### 5. Run the Emulator

```bash
ssh root@<deck-ip>
chmod +x /tmp/infones
/tmp/infones /tmp/your-game.nes < /dev/tty1
```

**Note:** The `< /dev/tty1` redirect is needed when running via SSH. If running directly from the Deck's terminal (fbterm), it's not required.

## Controls

| NES Button | Keyboard |
|------------|----------|
| D-Pad | Arrow keys or WASD |
| A | Z or J |
| B | X or K |
| Start | Enter |
| Select | Space or Shift |

## Getting NES ROMs

NES ROM files are not included. You can:

1. Use homebrew ROMs from [NESdev](https://www.nesdev.org/)
2. Dump your own cartridges
3. Use legally obtained ROM files

## Building from Source

Want to compile InfoNES yourself instead of using the pre-built binary? Check out [BUILD.md](BUILD.md) for complete instructions.

## License

InfoNES is freeware for non-commercial use. See the original InfoNES documentation for details.
