<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen-services/master/assets/lumen-services.svg" width="200" alt="Lumen">
</p>

# lumen-model-usage

Subscription usage monitoring for AI coding agents (Claude Code, Codex CLI): 5-hour session windows, weekly limits, model-specific limits, and credits.

[![Crates.io](https://img.shields.io/crates/v/lumen-model-usage)](https://crates.io/crates/lumen-model-usage)
[![docs.rs](https://img.shields.io/docsrs/lumen-model-usage)](https://docs.rs/lumen-model-usage)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Installation

```sh
cargo add lumen-model-usage
```

## Usage

```rust,no_run
use lumen_model_usage::{ModelUsageService, ProviderKind};
use tokio_stream::StreamExt;

async fn example() {
    let usage = ModelUsageService::builder()
        .providers(vec![ProviderKind::Claude, ProviderKind::Codex])
        .build();

    // Snapshot: read the latest usage data
    if let Some(snapshot) = usage.usage.get().as_ref() {
        for entry in &snapshot.providers {
            if let Ok(data) = &entry.result {
                for window in &data.windows {
                    println!(
                        "{} {}: {:.0}% remaining",
                        entry.kind.display_name(),
                        window.label,
                        window.remaining_percent()
                    );
                }
            }
        }
    }

    // Watch: update display when usage refreshes
    let mut stream = usage.usage.watch();
    while let Some(snapshot) = stream.next().await {
        if let Some(s) = snapshot.as_ref() {
            println!("Usage updated at {}", s.updated_at);
        }
    }
}
```

## How it works

The service reads the credential files the agent CLIs already maintain (`~/.claude/.credentials.json`, `~/.codex/auth.json`) — strictly read-only — and polls the same internal usage endpoints the CLIs' own `/usage` and `/status` screens use. Tokens are never refreshed by this crate; expired tokens are reported per provider and recover once the user runs the CLI again.

## License

MIT

Part of [lumen-services](https://github.com/lumen-rs/lumen-services).
