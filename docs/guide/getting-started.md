---
title: Getting started
---

# Getting started

Lumen is a Wayland desktop shell written in Rust with GTK4 and Relm4. It provides a top bar, notification center, on-screen display, wallpaper management and much more. Built-in mechanisms to handle audio, Bluetooth, network, media, battery and power controls.

Settings can be edited in `config.toml`, through the `lumen-settings` GUI, or with the `lumen config` CLI. All three surfaces provided hot reloaded updates, reflected in the shell immediately.

Lumen requires a Wayland compositor that implements the `wlr-layer-shell` protocol. Compositor-specific modules currently target Hyprland, Niri and Mango; Sway support is in development.

<a href="/lumen-preview.png" target="_blank" rel="noopener">
  <img src="/lumen-preview.png" alt="Lumen desktop shell" style="margin-bottom: 1.5rem;">
</a>

<a href="/lumen-settings-preview.png" target="_blank" rel="noopener">
  <img src="/lumen-settings-preview.png" alt="Lumen settings GUI">
</a>

## Install

- [Arch Linux](/guide/getting-started-arch)
- [Debian / Ubuntu](/guide/getting-started-debian)
- [Fedora](/guide/getting-started-fedora)
- [NixOS](/guide/getting-started-nixos)

<details>
<summary>Other distros</summary>

You'll need these libraries plus their `-dev` / `-devel` headers:

| Library          | Minimum version |
| ---------------- | --------------- |
| GTK              | 4.12            |
| gtk4-layer-shell | 1.0             |
| GtkSourceView    | 5               |
| libpulse         | 8               |
| fftw3            | 3               |
| libpipewire      | 0.3             |
| libudev          | any             |

Plus a C toolchain: `clang`, `cmake`, `pkg-config`, `git`, and a C/C++ compiler.

</details>
