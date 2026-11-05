#!/usr/bin/env sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
[ -f build/kanga.iso ] || sh scripts/build-iso.sh
exec qemu-system-x86_64 -machine q35 -m 256M -cdrom build/kanga.iso -serial stdio -vga std -no-reboot -no-shutdown
