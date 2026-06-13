<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen/master/assets/lumen.svg" width="200" alt="Lumen">
</p>

# lumen-traits

Shared traits for Lumen service monitoring and lifecycle.

[![Crates.io](https://img.shields.io/crates/v/lumen-traits)](https://crates.io/crates/lumen-traits)
[![docs.rs](https://img.shields.io/docsrs/lumen-traits)](https://docs.rs/lumen-traits)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

```sh
cargo add lumen-traits
```

## Usage

Implement `ServiceMonitoring` for background state watchers, `Static` for one-shot fetches, or `Reactive` for services that support both snapshot and live-updating access.

```rust,no_run
use lumen_traits::{Reactive, Static};
use std::sync::Arc;

struct MyService;

impl Static for MyService {
    type Error = String;
    type Context<'a> = &'a str;

    async fn get(context: Self::Context<'_>) -> Result<Self, Self::Error> {
        Ok(MyService)
    }
}
```

## License

MIT

Part of [Lumen](https://github.com/lumen-rs/lumen).
