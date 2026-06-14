# Processed upstream Wayle PRs

This file tracks upstream [`wayle-rs/wayle`](https://github.com/wayle-rs/wayle) pull requests that have been reviewed for Lumen and ported into this repository. Keep it updated so future upstream PR sweeps can skip already-processed work.

## Ported PRs

| Processed | Upstream PR | Impact | Original author | Lumen commit(s) | Notes |
|---|---:|---|---|---|---|
| 2026-06-14 | [wayle-rs/wayle#117](https://github.com/wayle-rs/wayle/pull/117) | Medium | `PerchunPak` / Perchun Pak | `1538cce6`, `575ad9bc` | Avoids overly generic application icon globs and keeps D-Bus matching behavior. |
| 2026-06-14 | [wayle-rs/wayle#282](https://github.com/wayle-rs/wayle/pull/282) | High | `zerbiniandrea` / zerby | `3f01b0ec` | Launches settings with a fresh GDK activation context so it opens on the active workspace. Adapted command/application names to Lumen. |
| 2026-06-14 | [wayle-rs/wayle#276](https://github.com/wayle-rs/wayle/pull/276) | High | `pigeonhands` / Sam M | `11a10ab3`, `f09dd9f1`, `75597601` | Shows hidden Niri workspaces correctly when `hide-trailing-empty` is disabled and removes the obsolete `min-workspace-count` config. |
| 2026-06-14 | [wayle-rs/wayle#264](https://github.com/wayle-rs/wayle/pull/264) | High | `pahnin` / Phanindra Srungavarapu | `5a6da176`, `6bc1d31c` | Applies dropdown opacity to the dropdown surface instead of the entire popover/content. |
| 2026-06-14 | [wayle-rs/wayle#253](https://github.com/wayle-rs/wayle/pull/253) | Medium | `pahnin` / Phanindra Srungavarapu | `983962fe`, `3b9af0b0`, `0f266096` | Persists and displays the selected theme preset in settings. |
| 2026-06-14 | [wayle-rs/wayle#261](https://github.com/wayle-rs/wayle/pull/261) | High | `adityapandeydev` / Aditya Pandey | `63df89c3` | Fixes settings GUI layout wipe, symlinked config hot-reload, and local CLI sibling binary resolution. Adapted binary names to Lumen. |
| 2026-06-14 | [wayle-rs/wayle#255](https://github.com/wayle-rs/wayle/pull/255) | High | `adityapandeydev` / Aditya Pandey | `cc44b182` | Fixes large dropdown rendering under fractional scaling. |
| 2026-06-14 | [wayle-rs/wayle#281](https://github.com/wayle-rs/wayle/pull/281) | Medium | `4fthawaiian` / bem | `dca29003`, `8ebb72c0` | Adds a config/settings option to disable the notifications service and module. |

## Selection criteria used

Only high/medium-impact PRs were ported in this batch. PRs with unresolved review concerns, draft status, major conflicts, unclear service dependencies, or risky runtime behavior were left out for a future design pass.

Each port keeps the original commit author where practical. Cherry-picked commits include Git's `(cherry picked from commit ...)` trailer, and this file records the original upstream PR and author for human-readable attribution.
