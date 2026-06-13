<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen-services/master/assets/lumen-services.svg" width="200" alt="Lumen">
</p>

# lumen-mango

Reactive MangoWM compositor state and event streaming.

[![Crates.io](https://img.shields.io/crates/v/lumen-mango)](https://crates.io/crates/lumen-mango)
[![docs.rs](https://img.shields.io/docsrs/lumen-mango)](https://docs.rs/lumen-mango)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

```sh
cargo add lumen-mango
```

## Usage

```rust,no_run
use futures::StreamExt;
use lumen_mango::MangoService;

async fn example() -> lumen_mango::Result<()> {
    let service = MangoService::new().await?;

    let layout = service.keyboard_layout.get();
    println!("layout: {layout:?}");

    let mut focused = service.focused_client.watch();
    while let Some(client) = focused.next().await {
        println!("focused: {client:?}");
    }
    Ok(())
}
```

State is exposed as [`Property`](https://docs.rs/lumen-core) values:

- `.get()` returns the current value.
- `.watch()` yields a `Stream` of changes.

Mango is dwm-derived, so each monitor has a fixed set of tags rather than a
growable workspace list. Read them from `service.monitors`.

## License

MIT

Part of [lumen-services](https://github.com/lumen-rs/lumen-services).
