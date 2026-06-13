<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen/master/assets/lumen.svg" width="200" alt="Lumen">
</p>

# lumen-battery

Battery monitoring via UPower D-Bus with reactive state updates.

[![Crates.io](https://img.shields.io/crates/v/lumen-battery)](https://crates.io/crates/lumen-battery)
[![docs.rs](https://img.shields.io/docsrs/lumen-battery)](https://docs.rs/lumen-battery)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

```sh
cargo add lumen-battery
```

## Usage

`BatteryService` monitors UPower's composite DisplayDevice by default. All device properties are reactive `Property<T>` types.

```rust,no_run
use lumen_battery::BatteryService;
use futures::StreamExt;

async fn example() -> Result<(), lumen_battery::Error> {
    let service = BatteryService::new().await?;

    let percentage = service.device.percentage.get();
    let state = service.device.state.get();
    println!("Battery: {percentage}% ({state})");

    let mut stream = service.device.state.watch();
    while let Some(new_state) = stream.next().await {
        println!("State changed: {new_state}");
    }
    Ok(())
}
```

For a specific battery, use `BatteryService::builder().device_path(path).build().await?`.

## License

MIT

Part of [Lumen](https://github.com/lumen-rs/lumen).
