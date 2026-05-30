.PHONY: all build check test test-fast clean install uninstall run release docs desktop help

BINARY = candor
PREFIX ?= /usr/local

all: build

help:
	@echo "Candor AI — Makefile"
	@echo ""
	@echo "  build       Build release binary"
	@echo "  check       Fast compile check"
	@echo "  test        Run all tests (may take 90s)"
	@echo "  test-fast   Run quick tests only (no slow SurrealDB)"
	@echo "  install     Install binary to \$$PREFIX/bin"
	@echo "  run         Run the daemon"
	@echo "  chat        Start interactive chat"
	@echo "  health      Check system health"
	@echo "  clean       Clean build artifacts"
	@echo "  docs        Build documentation"
	@echo "  desktop     Start desktop UI (requires npm)"
	@echo ""

build:
	cargo build --release
	@echo "\nBinary: target/release/$(BINARY)"

check:
	cargo check --workspace

test:
	cargo test

test-fast:
	cargo test --test edge_cases -p candor-tools
	cargo test -p candor-cognitive
	cargo test -p candor-core
	cargo test -p candor-graph
	cargo test -p candor-sentinel
	cargo test -p candor-tools
	cargo test -p candor-daemon

install: build
	cp target/release/$(BINARY) $(PREFIX)/bin/$(BINARY)
	@echo "Installed to $(PREFIX)/bin/$(BINARY)"

uninstall:
	rm -f $(PREFIX)/bin/$(BINARY)

run:
	cargo run

chat:
	cargo run -- --chat

health:
	cargo run -- --health

clean:
	cargo clean
	rm -rf desktop/dist desktop/node_modules desktop/src-tauri/target

docs:
	@echo "See docs/ directory for documentation"
	@ls docs/ -R

desktop:
	cd desktop && npm install && npm run tauri dev
