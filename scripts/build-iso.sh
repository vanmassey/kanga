#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

LIMINE_VERSION=12.9.6
LIMINE_SHA256=b38aabb042b0c0f9f562b7741be3c1204838d7bbacfa78bb411c674bd5b984ee
LIMINE_ARCHIVE="build/limine-binary.tar.gz"
LIMINE_URL="https://github.com/Limine-Bootloader/Limine/releases/download/v${LIMINE_VERSION}/limine-binary.tar.gz"

mkdir -p build
if [ ! -f "$LIMINE_ARCHIVE" ]; then
    curl -fL "$LIMINE_URL" -o "$LIMINE_ARCHIVE"
fi
printf '%s  %s\n' "$LIMINE_SHA256" "$LIMINE_ARCHIVE" | sha256sum -c -

mkdir -p build/limine
tar -xzf "$LIMINE_ARCHIVE" -C build/limine

find_file() {
    find build/limine -type f -name "$1" -print -quit
}

BIOS_SYS=$(find_file limine-bios.sys)
BIOS_CD=$(find_file limine-bios-cd.bin)
UEFI_CD=$(find_file limine-uefi-cd.bin)
UEFI_APP=$(find_file BOOTX64.EFI)

if [ -z "$BIOS_SYS" ]; then
    echo "Limine BIOS system file is missing" >&2
    exit 1
fi
LIMINE_DIR=$(dirname "$BIOS_SYS")
make -C "$LIMINE_DIR"
LIMINE_TOOL="$LIMINE_DIR/limine"

for file in "$BIOS_CD" "$UEFI_CD" "$UEFI_APP" "$LIMINE_TOOL"; do
    if [ -z "$file" ] || [ ! -f "$file" ]; then
        echo "Required Limine binary missing: $file" >&2
        exit 1
    fi
done

cargo build -Z build-std=core,compiler_builtins

ISO_ROOT=build/iso-root
rm -rf "$ISO_ROOT"
mkdir -p "$ISO_ROOT/boot/limine" "$ISO_ROOT/EFI/BOOT"
cp target/x86_64-unknown-none/debug/kanga-kernel "$ISO_ROOT/boot/kanga.elf"
cp limine.conf "$ISO_ROOT/boot/limine/limine.conf"
cp "$BIOS_SYS" "$BIOS_CD" "$UEFI_CD" "$ISO_ROOT/boot/limine/"
cp "$UEFI_APP" "$ISO_ROOT/EFI/BOOT/BOOTX64.EFI"

xorriso -as mkisofs -R -r -J -hfsplus -apm-block-size 2048 \
    -partition_offset 16 \
    -b boot/limine/limine-bios-cd.bin \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    --efi-boot boot/limine/limine-uefi-cd.bin \
    --protective-msdos-label \
    "$ISO_ROOT" -o build/kanga.iso

"$LIMINE_TOOL" bios-install build/kanga.iso
printf 'Created %s\n' "$ROOT/build/kanga.iso"
