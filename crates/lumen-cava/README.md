<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen-services/master/assets/lumen-services.svg" width="200" alt="Lumen">
</p>

# lumen-cava

Real-time audio frequency visualization via libcava.

[![Crates.io](https://img.shields.io/crates/v/lumen-cava)](https://crates.io/crates/lumen-cava)
[![docs.rs](https://img.shields.io/docsrs/lumen-cava)](https://docs.rs/lumen-cava)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

```sh
cargo add lumen-cava
```

## Usage

`CavaService` captures system audio and produces frequency bar amplitudes. The `values` field updates at the configured framerate (default 60fps).

```rust,no_run
use lumen_cava::{CavaService, InputMethod};
use futures::StreamExt;

async fn example() -> Result<(), lumen_cava::Error> {
    let cava = CavaService::builder()
        .bars(32)
        .framerate(30)
        .input(InputMethod::PipeWire)
        .build()
        .await?;

    let mut stream = cava.values.watch();
    while let Some(values) = stream.next().await {
        println!("Bars: {:?}", &values[..4]);
    }
    Ok(())
}
```

Runtime changes like `set_bars()` and `set_noise_reduction()` restart the capture automatically. The `vendored` feature (on by default) compiles libcava from source.

## License

MIT

Part of [lumen-services](https://github.com/lumen-rs/lumen-services).
