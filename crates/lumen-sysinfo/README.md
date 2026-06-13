<p align="center">
  <img src="https://raw.githubusercontent.com/lumen-rs/lumen-services/master/assets/lumen-services.svg" width="200" alt="Lumen">
</p>

# lumen-sysinfo

CPU, memory, disk, and network metrics via polling-based background tasks.

[![Crates.io](https://img.shields.io/crates/v/lumen-sysinfo)](https://crates.io/crates/lumen-sysinfo)
[![docs.rs](https://img.shields.io/docsrs/lumen-sysinfo)](https://docs.rs/lumen-sysinfo)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Installation

```sh
cargo add lumen-sysinfo
```

## Usage

```rust,no_run
use lumen_sysinfo::SysinfoService;
use futures::StreamExt;

async fn example() {
    let service = SysinfoService::builder().build();

    let cpu = service.cpu.get();
    println!("CPU: {:.1}%", cpu.usage_percent);

    let memory = service.memory.get();
    println!("Memory: {:.1}%", memory.usage_percent);

    let mut stream = service.cpu.watch();
    while let Some(cpu) = stream.next().await {
        println!("CPU changed: {:.1}%", cpu.usage_percent);
    }
}
```

## License

MIT

Part of [lumen-services](https://github.com/lumen-rs/lumen-services).
