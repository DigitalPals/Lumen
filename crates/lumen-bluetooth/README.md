<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen/master/assets/lumen.svg" width="200" alt="Lumen">
</p>

# lumen-bluetooth

Bluetooth device management and discovery via BlueZ D-Bus.

[![Crates.io](https://img.shields.io/crates/v/lumen-bluetooth)](https://crates.io/crates/lumen-bluetooth)
[![docs.rs](https://img.shields.io/docsrs/lumen-bluetooth)](https://docs.rs/lumen-bluetooth)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

```sh
cargo add lumen-bluetooth
```

## Usage

`BluetoothService` exposes adapter state, paired devices, and discovery controls. All fields are reactive `Property<T>` types.

```rust,no_run
use lumen_bluetooth::BluetoothService;
use futures::StreamExt;

async fn example() -> Result<(), lumen_bluetooth::Error> {
    let bt = BluetoothService::new().await?;

    // Snapshot: list currently paired devices
    for device in bt.devices.get().iter() {
        let name = device.alias.get();
        println!("{name}: connected={}", device.connected.get());
    }

    // Watch: log when any device connects or disconnects
    let mut stream = bt.devices.watch();
    while let Some(devices) = stream.next().await {
        for device in devices.iter() {
            if device.connected.get() {
                println!("{} connected", device.alias.get());
            }
        }
    }
    Ok(())
}
```

Devices support `connect()`, `disconnect()`, `pair()`, and `forget()` operations.

## License

MIT

Part of [Lumen](https://github.com/lumen-rs/lumen).
