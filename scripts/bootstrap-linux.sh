#!/bin/bash
# Installs the system packages Snow Shot needs on Debian/Ubuntu.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
if [[ "${1:-}" == --help ]]; then
    echo 'Usage: bootstrap-linux.sh [--skip-dependency-install]'
    exit 0
fi
if [[ "${1:-}" != --skip-dependency-install ]]; then
    packages=(
        build-essential cmake ninja-build pkg-config
        qt6-base-dev qt6-base-private-dev qt6-svg-dev qt6-tools-dev qt6-wayland-dev
        qt6-l10n-tools libgl-dev
        libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev libswresample-dev
        libpipewire-0.3-dev
        libx11-dev libxtst-dev libx11-xcb-dev libxcb-xfixes0-dev
        libdbus-1-dev libatspi2.0-dev
        libopencv-dev libminizip-ng-dev
        libheif-dev libjxl-dev libgif-dev libtinyxml2-dev libcups2-dev
        clang-format nasm
    )
    if [[ ! -f /opt/libjpeg-turbo-3.1.1/lib/cmake/libjpeg-turbo/libjpeg-turboConfig.cmake ]]; then
        echo "TurboJPEG 3.1 is required. Install libjpeg-turbo 3.1.1 under /opt/libjpeg-turbo-3.1.1." >&2
        exit 1
    fi
    sudo apt-get install -y "${packages[@]}"
fi
command -v cmake >/dev/null
command -v ninja >/dev/null
command -v rustc >/dev/null
command -v pkg-config >/dev/null
pkg-config --exists Qt6Core libavcodec libpipewire-0.3 opencv4
printf 'Linux build dependencies are available in %s\n' "$root"
