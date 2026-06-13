<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen-services/master/assets/lumen-services.svg" width="200" alt="Lumen">
</p>

# lumen-brightness

Backlight control for internal displays with reactive state.

[![Crates.io](https://img.shields.io/crates/v/lumen-brightness)](https://crates.io/crates/lumen-brightness)
[![docs.rs](https://img.shields.io/docsrs/lumen-brightness)](https://docs.rs/lumen-brightness)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

```sh
cargo add lumen-brightness
```

## Usage

`BrightnessService::new()` returns `None` when no backlight devices are found. The `primary` field tracks the main display's backlight device.

```rust,no_run
use lumen_brightness::{BrightnessService, Percentage};
use futures::StreamExt;

async fn example() -> Result<(), lumen_brightness::Error> {
    let Some(brightness) = BrightnessService::new().await? else {
        return Ok(());
    };

    if let Some(device) = brightness.primary.get() {
        println!("{}: {}", device.name, device.percentage());
        device.set_percentage(Percentage::new(50.0)).await?;
    }

    let mut stream = brightness.primary.watch();
    while let Some(maybe_device) = stream.next().await {
        if let Some(device) = maybe_device {
            println!("Brightness: {}", device.percentage());
        }
    }
    Ok(())
}
```

On non-systemd systems, direct sysfs writes require `video` group membership.

## License

MIT

Part of [lumen-services](https://github.com/lumen-rs/lumen-services).
