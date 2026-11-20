#!/usr/bin/env bash
set -euo pipefail

prefix=()
if [[ "$(id -u)" != 0 ]]; then
    prefix=(sudo)
fi

"${prefix[@]}" apt-get update
"${prefix[@]}" apt-get install -y --no-install-recommends \
    build-essential pkg-config cmake \
    libfontconfig1-dev libfreetype6-dev libssl-dev libzstd-dev \
    libvulkan-dev mesa-vulkan-drivers fonts-dejavu-core \
    libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols \
    libx11-dev libxcb1-dev libxcb-randr0-dev libxcb-shape0-dev \
    libxcb-xfixes0-dev libxcb-xkb-dev libxcb-render0-dev libxcb-shm0-dev libxcb-sync-dev
