---
title: Getting started on Arch
---

# Getting started on Arch Linux

## Prebuilt (fastest path)

`lumen-bin` from the AUR ships both binaries prebuilt. If you take this route, skip the build step below and jump straight to [Run](#run).

```sh
yay -S lumen-bin
```

## Install dependencies

Install Rust via [rustup](https://rustup.rs), then the system libraries:

```sh
sudo pacman -S --needed git gtk4 gtk4-layer-shell gtksourceview5 \
  libpulse fftw libpipewire systemd-libs clang base-devel
```

If you want the battery, bluetooth, network, power, or audio modules, install their daemons (skip any you don't need):

```sh
sudo pacman -S --needed bluez bluez-utils networkmanager upower \
  power-profiles-daemon pipewire wireplumber pipewire-pulse
sudo systemctl enable --now bluetooth NetworkManager upower power-profiles-daemon
```

## Build

```sh
git clone https://github.com/lumen-rs/lumen
cd lumen
cargo install --path lumen
cargo install --path crates/lumen-settings
```

## Icon assets

Lumen ships icons as source files that get copied into your user data directory on first setup. Run this from the cloned repo, **before** deleting it:

```sh
lumen icons setup
```

## Run

Start the panel in the background:

```sh
lumen panel start
```

Other lifecycle commands: `lumen panel status`, `lumen panel restart`, `lumen panel stop`.

For debugging, run the shell in the foreground so logs print to the terminal:

```sh
lumen shell
```

## Settings GUI

```sh
lumen panel settings
```

This launches `lumen-settings`, which edits the same config the shell reads. Changes apply live. Anything the GUI doesn't cover can still be edited by hand in `config.toml`.

## Configuration

Lumen reads `$XDG_CONFIG_HOME/lumen/config.toml`, falling back to `~/.config/lumen/config.toml`. On first run, if no config exists, Lumen writes a default one. A JSON schema is written to `~/.config/lumen/schema.json` at startup, which editors with a TOML LSP can use for validation and completion.
