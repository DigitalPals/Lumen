<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen/master/assets/lumen.svg" width="200" alt="Lumen">
</p>

# lumen-systray

System tray management via the StatusNotifier (SNI) and DBusMenu protocols.

[![Crates.io](https://img.shields.io/crates/v/lumen-systray)](https://crates.io/crates/lumen-systray)
[![docs.rs](https://img.shields.io/docsrs/lumen-systray)](https://docs.rs/lumen-systray)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Installation

```sh
cargo add lumen-systray
```

## Usage

```rust,no_run
use lumen_systray::SystemTrayService;
use futures::StreamExt;

async fn example() -> Result<(), lumen_systray::error::Error> {
    let service = SystemTrayService::new().await?;

    for item in service.items.get().iter() {
        println!("{}: {}", item.id.get(), item.title.get());
    }

    let mut stream = service.items.watch();
    while let Some(items) = stream.next().await {
        println!("{} tray items", items.len());
    }

    Ok(())
}
```

## License

MIT

Part of [Lumen](https://github.com/lumen-rs/lumen).
