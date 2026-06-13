<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen/master/assets/lumen.svg" width="200" alt="Lumen">
</p>

# lumen-audio

Reactive PulseAudio integration for managing audio devices and streams.

[![Crates.io](https://img.shields.io/crates/v/lumen-audio)](https://crates.io/crates/lumen-audio)
[![docs.rs](https://img.shields.io/docsrs/lumen-audio)](https://docs.rs/lumen-audio)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

```sh
cargo add lumen-audio
```

## Usage

All fields on `AudioService` are reactive `Property<T>` types with `.get()` for snapshots and `.watch()` for change streams.

```rust,no_run
use lumen_audio::AudioService;
use futures::StreamExt;

async fn example() -> Result<(), lumen_audio::Error> {
    let audio = AudioService::new().await?;

    if let Some(device) = audio.default_output.get() {
        println!("Output: {}", device.description.get());
        println!("Muted: {}", device.muted.get());
        device.set_mute(true).await?;
    }

    let mut stream = audio.default_output.watch();
    while let Some(maybe_device) = stream.next().await {
        match maybe_device {
            Some(device) => println!("Default output: {}", device.description.get()),
            None => println!("No default output device"),
        }
    }
    Ok(())
}
```

Use `AudioService::builder().with_daemon().build().await?` to expose the service over D-Bus at `com.lumen.Audio1`.

## License

MIT

Part of [Lumen](https://github.com/lumen-rs/lumen).
