.PHONY: all build start dev suggest test check lint fmt coverage audit install

HISTORY ?= slate:xxxxx

all: build

build:
	cargo build --release

start:
	cargo run --release

dev:
	cargo run

suggest:
	cargo run --release -- suggest --history $(HISTORY)

test:
	cargo test --release

check: fmt lint test

lint:
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt --all -- --check

coverage:
	cargo llvm-cov --release --lcov --output-path lcov.info

audit:
	cargo audit

install:
	cargo install --path . --locked
