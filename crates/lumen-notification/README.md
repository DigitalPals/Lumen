<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen/master/assets/lumen.svg" width="200" alt="Lumen">
</p>

# lumen-notification

Desktop notification service implementing the freedesktop.org Desktop Notifications spec.

[![Crates.io](https://img.shields.io/crates/v/lumen-notification)](https://crates.io/crates/lumen-notification)
[![docs.rs](https://img.shields.io/docsrs/lumen-notification)](https://docs.rs/lumen-notification)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Installation

```sh
cargo add lumen-notification
```

## Usage

```rust,no_run
use lumen_notification::NotificationService;
use futures::StreamExt;

async fn example() -> Result<(), lumen_notification::Error> {
    let service = NotificationService::new().await?;

    let count = service.notifications.get().len();
    println!("{count} notifications");

    let mut stream = service.notifications.watch();
    while let Some(notifications) = stream.next().await {
        println!("{} notifications", notifications.len());
    }

    Ok(())
}
```

## License

MIT

Part of [Lumen](https://github.com/lumen-rs/lumen).
