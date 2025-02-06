.PHONY: test
test:
	cargo test --workspace -- --nocapture
	cargo test --test integration_test -- --nocapture