.PHONY: lint test clean

ifeq ($(shell uname -s),Darwin)
LINT_EXCLUDE = --exclude x11-tests
endif

lint:
	cargo clippy --workspace $(LINT_EXCLUDE) --all-targets -- --deny warnings

test:
	cargo test --workspace --all-features

clean:
	cargo clean
