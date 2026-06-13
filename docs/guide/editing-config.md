---
title: Editing config
---

# Editing config

Lumen reads configuration from `~/.config/lumen/config.toml`. All fields have defaults, so `config.toml` may be empty; declare only the fields that should override a default.

The `lumen config` CLI and the settings GUI write to `~/.config/lumen/runtime.toml` rather than `config.toml`. For each field, Lumen uses the first value defined among these sources:

1. `runtime.toml` - overrides written by `lumen config` or the settings GUI.
2. `config.toml` - values declared by hand.
3. The built-in default.

A minimal override:

```toml
[bar]
scale = 1.25

[modules.clock]
format = "%H:%M"
```

Every supported key is documented in the [config reference](/config/).

## Imports

`config.toml` may declare a top-level `imports` array to load additional TOML files. Files referenced through `imports` may themselves declare `imports`, forming a chain:

```toml
imports = ["themes/nord.toml", "modules/clock.toml"]

[bar]
scale = 1.25
```

Paths are resolved relative to the importing file's directory; the `.toml` extension may be omitted. Imports are merged in declaration order, then the importing file is overlaid on top. Tables merge key by key; scalars and arrays in the overlay replace the corresponding value in the base. The merged result becomes the `config.toml` layer described above.

`runtime.toml` does not resolve imports. Circular chains are rejected at load; the previous valid configuration remains active and the error is recorded in the log.

## Editor setup

On startup, Lumen writes a JSON Schema for the configuration to `~/.config/lumen/schema.json`. Any TOML language server with JSON Schema support can use this file for completion, hover documentation, and validation. The schema is generated from the installed binary and matches the version of Lumen on disk.

[Tombi](https://tombi-toml.github.io/tombi/) is one such server. The Tombi extension is available in the VS Code marketplace; the `tombi` LSP binary runs under Neovim, Helix, and Zed. Configure the server to associate `~/.config/lumen/schema.json` with `config.toml`.

## Live reload

Lumen watches the configuration directory. Changes to `config.toml` trigger an in-process reload; a shell restart is not required. Invalid configuration is rejected, the previous valid state is retained, and parse or validation errors are recorded in the log.

## Editing from the CLI

The `lumen config` subcommand reads and writes individual fields by dotted path:

```bash
lumen config get bar.scale
lumen config set modules.clock.format "%H:%M"
lumen config reset modules.clock.format
```

`set` writes to `~/.config/lumen/runtime.toml`; `config.toml` is never modified by the CLI or GUI Settings dialog. `reset` removes the runtime override for the given path, reverting the field to the value declared in `config.toml` or to the built-in default.

## Printing the default configuration

`lumen config default --stdout` prints every key with its default value to standard output. Without `--stdout`, the command writes `config.toml.example` to the configuration directory; `config.toml` is not modified. `lumen config schema --stdout` prints the JSON Schema in the same manner.
