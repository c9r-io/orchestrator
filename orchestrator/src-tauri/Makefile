COVERAGE_MIN ?= 90

.PHONY: build build-ui test test-ui coverage coverage-html

build:
	cargo build

build-ui:
	cargo build --features ui

test:
	cargo test --lib --bins

test-ui:
	cargo test --lib --bins --features ui

coverage:
	cargo llvm-cov --lib --fail-under-lines $(COVERAGE_MIN) --ignore-filename-regex 'build.rs'

coverage-html:
	cargo llvm-cov --lib --html --fail-under-lines $(COVERAGE_MIN) --ignore-filename-regex 'build.rs'
