.PHONY: build check clean

build:
	cargo build -Z build-std=core,compiler_builtins

check:
	cargo check -Z build-std=core,compiler_builtins

clean:
	cargo clean
