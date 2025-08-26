all:
	cargo build --release

run:
	cargo run --release

test:
	cargo test -- --show-output

.PHONY: test run
