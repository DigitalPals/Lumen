<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen-services/master/assets/lumen-services.svg" width="200" alt="Lumen">
</p>

# lumen-media

MPRIS media player control and playback tracking via D-Bus.

[![Crates.io](https://img.shields.io/crates/v/lumen-media)](https://crates.io/crates/lumen-media)
[![docs.rs](https://img.shields.io/docsrs/lumen-media)](https://docs.rs/lumen-media)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Installation

```sh
cargo add lumen-media
```

## Usage

```rust,no_run
use lumen_media::MediaService;
use futures::StreamExt;

async fn example() -> Result<(), lumen_media::Error> {
    let media = MediaService::new().await?;

    if let Some(player) = media.active_player.get() {
        println!("{}: {}", player.identity.get(), player.metadata.title.get());
    }

    let mut stream = media.active_player.watch();
    while let Some(player) = stream.next().await {
        match player {
            Some(p) => println!("{} playing: {}", p.identity.get(), p.metadata.title.get()),
            None => println!("No active player"),
        }
    }
    Ok(())
}
```

## License

MIT

Part of [lumen-services](https://github.com/lumen-rs/lumen-services).
