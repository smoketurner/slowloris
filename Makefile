# Convenience wrappers around cargo.
#
#   make build            release build
#   make run ARGS="..."   build and run, passing ARGS to the binary
#   make clean            remove build artifacts
#
# Example:
#   make run ARGS="http://127.0.0.1:8080/ -c 200 -l 30"

CARGO ?= cargo
ARGS  ?=

.PHONY: build run clean

build:
	$(CARGO) build --release

run:
	$(CARGO) run --release -- $(ARGS)

clean:
	$(CARGO) clean
