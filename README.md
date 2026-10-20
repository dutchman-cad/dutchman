# Dutchman

Dutchman carries CAD designs between formats: it decodes native design files into open,
editable models. Decoding is done by [cadmpeg](https://github.com/cadmpeg/cadmpeg); Dutchman
maps the result onto a target format, starting with [SolidCraft](https://github.com/tangency-labs/solidcraft)
designs.

Early development: Fusion `.f3d` parts convert to `.solidcraft` designs as a non-parametric
imported body. Feature history comes next.

```sh
dutchman convert part.f3d part.solidcraft
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together.

## Building

```sh
cargo build --release
cargo test --workspace
```

## Licence

Licensed under the [Apache License, Version 2.0](LICENSE).
