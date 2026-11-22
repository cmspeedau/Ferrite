<p align="center">
  <p align="center">
    <img src="https://repository-images.githubusercontent.com/635377565/a694ba10-40cb-4c7e-aec9-c3d44516e6c7" style="width: 15%;" alt="wmlogo"></img>
    <h1 align="center">Ferrite</h1>
 <h3 align="center">a versatile and customizable Wayland Compositor</h3>
</p>
</p>
  <p align="center">
    <img src="https://img.shields.io/github/languages/top/cmspeedau/Ferrite?style=for-the-badge"/>
    <img src="https://img.shields.io/github/commit-activity/m/cmspeedau/Ferrite?style=for-the-badge"/>
    <img src="https://img.shields.io/github/issues/cmspeedau/Ferrite?style=for-the-badge"/>
    <img src="https://img.shields.io/github/license/cmspeedau/Ferrite?style=for-the-badge"/>
    <img src="https://img.shields.io/discord/1087402623646322748?style=for-the-badge"/>
  </p>

## About

**Ferrite** is a versatile and customizable window manager / [Wayland compositor](https://wayland.freedesktop.org/), currently in development and actively seeking contributions from the community. Built with the [Smithay](https://github.com/Smithay/smithay) library and programmed in [Rust](https://www.rust-lang.org/), Ferrite along with it's RON config provides users with flexibility and customization options. Ferrite is licensed under MIT, ensuring that it remains open-source and free for all to use and contribute to.

Join our [Discord](https://discord.gg/VM8DkxaHfa)!

## Features

- [x] RON Configuration
- [x] Nvidia Support
- [ ] HDR Support (see [docs/hdr.md](docs/hdr.md))
- [x] Dynamic Tiling and Floating Windows
- [x] Keyboard and Monitor Managament
- [x] [Screencopy](https://wayland.app/protocols/wlr-screencopy-unstable-v1) Protocols for Screensharing/Screenshots
- [x] Blur
- [x] Animations
- [X] Borders
- [x] Can display wayland applications
- [x] Xwayland Support
- [x] Working Popups
- [x] Can be launched from TTY
- [x] Dock / panel (see [docs/dock-mockup.html](docs/dock-mockup.html) for design reference, [docs/theme.md](docs/theme.md) for the Satisfactory colour theme)
- [ ] FICSIT-style TUI login screen (Satisfactory-inspired greeter shown before session start; design mockup in chat — needs final copy pass, e.g. fix "MASSAGE-2(A-B)b" sector label, confirm font/box-drawing glyph coverage)

<!-- hello there -->

## Build

### 1. Dependencies
You will need to install Ferrite's dependencies with your package manager of choice.

#### Debian and derivatives (Ubuntu, Linux Mint, MX Linux, etc.)
```bash
# apt install libudev-dev libgbm-dev libxkbcommon-dev libegl1-mesa-dev libwayland-dev libinput-dev libdbus-1-dev libsystemd-dev libseat-dev
```

#### Arch and derivatives (EndeavourOS, Garuda, etc.)
> **Manjaro is not supported.**
```bash
# pacman -Syu udev wayland wayland-protocols libinput libxkbcommon libglvnd seatd dbus-glib mesa
```

#### Fedora
```bash
# dnf install systemd-devel libgbm-devel libxkbcommon-devel Mesa-libEGL-devel wayland-devel libinput-devel dbus-glib-devel libseat-devel
```

#### openSUSE Tumbleweed
```bash
# zypper in systemd-devel libgbm-devel libxkbcommon-devel Mesa-libEGL1 wayland-devel libinput-devel libdbus-glib-1-3 seatd-devel
```

### 2. Compilation
Clone the git repo and build Ferrite by running the following command:
```bash
$ cargo build --release
```
The binary will be created in `./target/release/ferrite`.
> 💡 You can also use `cargo run --release` to run the project.
## Install
**Ferrite** is still under heavy development and installation is not recommended.
If you really want to, run the following command to install Ferrite: 
```bash
cargo install --path .
```

## Troubleshooting

### Getting logs
Logs for Ferrite can be found at `$HOME/.local/share/Ferrite/`, when debugging a issue run Ferrite with `RUST_LOG=debug`
