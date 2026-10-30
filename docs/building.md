# Building

Dutchman is pure Rust, Apache-2.0. Native files are read by
[cadmpeg](https://github.com/cadmpeg/cadmpeg); Dutchman maps its neutral model onto
[SolidCraft](https://github.com/tangency-labs/solidcraft).

Use Rust 1.88 or newer.

```sh
cargo build --release --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

For packagers: the std-only `dutchman-launcher` crate provides a tiny front command for installations that
ship with Pilot alone. Packagers install its binary as `dutchman`; `dutchman --version`
identifies it as `dutchman launcher <version>`. Install Dutchman Convert for conversion commands.

See [Contributing](../CONTRIBUTING.md#building-and-checking) for the required checks and
[Architecture](ARCHITECTURE.md) for the pipeline and crate boundaries.
