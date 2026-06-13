<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen-services/master/assets/lumen-services.svg" width="200" alt="Lumen">
</p>

# lumen-core

Reactive state primitives and D-Bus utilities shared across Lumen services.

[![Crates.io](https://img.shields.io/crates/v/lumen-core)](https://crates.io/crates/lumen-core)
[![docs.rs](https://img.shields.io/docsrs/lumen-core)](https://docs.rs/lumen-core)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

```sh
cargo add lumen-core
```

## Usage

Wrap any value in a `Property<T>` to get snapshot reads and change streams.

```rust,no_run
use lumen_core::Property;
use futures::stream::StreamExt;

async fn example() {
    let brightness = Property::new(75u32);
    brightness.set(100);

    let mut changes = brightness.watch();
    while let Some(level) = changes.next().await {
        println!("{level}");
    }
}
```


## Features

- `schema` enables `schemars::JsonSchema` support on `Property<T>`

## License

MIT

Part of [lumen-services](https://github.com/lumen-rs/lumen-services).
