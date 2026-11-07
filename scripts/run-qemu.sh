#!/usr/bin/env sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
[ -f build/kanga.iso ] || sh scripts/build-iso.sh
# Expose the host-independent maximum x86 feature set to Limine, including
# 1 GiB page support, rather than QEMU's more limited default CPU model.
exec qemu-system-x86_64 -machine q35 -cpu max -m 256M -cdrom build/kanga.iso -serial stdio -vga std -no-reboot -no-shutdown
