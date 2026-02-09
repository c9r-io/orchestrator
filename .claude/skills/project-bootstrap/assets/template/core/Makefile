.PHONY: test coverage coverage-html

test:
	cargo test

# Requires cargo-llvm-cov:
#   cargo install cargo-llvm-cov
coverage:
	cargo llvm-cov

coverage-html:
	cargo llvm-cov --html
