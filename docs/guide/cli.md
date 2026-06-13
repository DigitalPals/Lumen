# CLI

Every subcommand takes `--help`.

Panel lifecycle:

```sh
lumen panel start
lumen panel restart
lumen panel settings
```

Read and edit config values from the command line:

```sh
lumen config get bar.scale
lumen config set bar.scale 1.25
lumen config reset bar.scale
```

Audio, media, and idle controls:

```sh
lumen audio output-volume +5
lumen media play-pause
lumen idle toggle
```

Shell completions for bash, fish, and zsh:

```sh
lumen completions fish > ~/.config/fish/completions/lumen.fish
```
