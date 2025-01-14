requirement:
	@echo "Installing Rust"
	@curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	@export PATH="$HOME/.cargo/bin:$PATH"

cargo-install:
	@echo "Installing recipe-pick"
	@cargo install --path .

install: requirement cargo-install
