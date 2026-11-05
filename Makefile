.PHONY: build check iso run clean

build:
	cargo build -Z build-std=core,compiler_builtins

check:
	cargo fmt --all -- --check
	cargo check -Z build-std=core,compiler_builtins

iso:
	sh scripts/build-iso.sh

run: iso
	sh scripts/run-qemu.sh

clean:
	cargo clean
	rm -rf build
