<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen-services/master/assets/lumen-services.svg" width="200" alt="Lumen">
</p>

# lumen-power-profiles

Power profile switching and monitoring via power-profiles-daemon D-Bus.

[![Crates.io](https://img.shields.io/crates/v/lumen-power-profiles)](https://crates.io/crates/lumen-power-profiles)
[![docs.rs](https://img.shields.io/docsrs/lumen-power-profiles)](https://docs.rs/lumen-power-profiles)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Installation

```sh
cargo add lumen-power-profiles
```

## Usage

```rust,no_run
use lumen_power_profiles::PowerProfilesService;
use futures::StreamExt;

async fn example() -> Result<(), lumen_power_profiles::Error> {
    let service = PowerProfilesService::new().await?;

    // Snapshot: check the current profile
    let profile = service.power_profiles.active_profile.get();
    println!("Current profile: {profile}");

    // Watch: log whenever the power profile switches
    let mut stream = service.power_profiles.active_profile.watch();
    while let Some(profile) = stream.next().await {
        println!("Profile switched to: {profile}");
    }
    Ok(())
}
```

## License

MIT

Part of [lumen-services](https://github.com/lumen-rs/lumen-services).
