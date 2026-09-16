.PHONY: gate

gate:
	nix develop --command cargo build
	nix develop --command cargo test --workspace
	nix develop --command cargo clippy --workspace --all-targets -- -D warnings
	nix develop --command cargo fmt --check
