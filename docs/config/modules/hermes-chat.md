---
title: hermes-chat
outline: [2, 3]
---

# hermes-chat

<div v-pre>

Hermes Agent chat dropdown backed by an external Hermes API server.

Add it to your layout with `hermes-chat`:

```toml
[[bar.layout]]
monitor = "*"
right = ["hermes-chat"]
```

Hermes tools run on the **Hermes backend host**, not on the Lumen desktop client. For API/SSE transports, start Hermes with `API_SERVER_ENABLED=true`, set `API_SERVER_KEY=[REDACTED]`, and point `api-key` at a `$VAR_NAME` such as `$HERMES_API_SERVER_KEY`. For the Desktop/TUI dashboard transport, point `dashboard-token` at `$HERMES_DESKTOP_REMOTE_TOKEN`; local loopback dashboards can also expose the token from their root page. In `auto` mode, hermes-chat tries the dashboard transport first and falls back to API/SSE compatibility probes when no dashboard is available.

## Chat usage

The dropdown shows recent Hermes sessions, highlights active and waiting-for-input sessions in a compact activity panel with preview text, marks the current session, and renders assistant responses with markdown, tool activity, live todo/task status, live subagent/delegated-task status, live background process status with expandable output and stop/dismiss controls, approvals, clarifications, masked sudo/secret prompts, live reasoning when Hermes provides it, and persistent backend review summaries. Running background process rows refresh periodically while the dropdown is open.

In the composer, `Enter` submits the current message or accepts the highlighted slash suggestion, `Shift+Enter` inserts a newline, `Up`/`Down` select slash suggestions when the suggestion list is open and otherwise browse previous single-line prompts, and `Escape` clears the live slash-command suggestions. Choosing a bare arg-taking slash command, such as `/skin`, `/resume`, `/handoff`, `/browser`, `/personality`, or `/tools`, expands it to its argument step instead of submitting it immediately. Draft text is scoped to the active Hermes session and is restored when you switch back to that session. Normal prompts submitted while Hermes is busy are queued for the active session and drain when Hermes becomes ready again; slash commands still run immediately.

Local slash commands handled by the dropdown:

| Command | Description |
|---|---|
| `/new`, `/reset` | Start a new chat. |
| `/branch`, `/fork` | Branch the latest user or assistant message into a new dashboard chat. |
| `/browser [connect\|disconnect\|status] [url]` | Manage the local gateway browser CDP connection. |
| `/handoff <platform>` | Hand off the active session to a messaging platform. |
| `/profile [list\|name]` | List dashboard profiles or set the profile used for new dashboard chats. |
| `/skin [list\|next\|name]` | List, cycle, or apply a Lumen theme preset. |
| `/sessions` | Show loaded recent sessions, active work, and input waits. |
| `/resume <id, title, or preview>`, `/switch <id, title, or preview>` | Switch to a loaded session, or open a stored dashboard session by id. |
| `/title <name>` | Rename the current dashboard session through Hermes. |
| `/yolo` | Toggle per-session YOLO approval bypass for the current dashboard session. |
| `/help`, `/commands` | Show the Hermes backend command catalog when the dashboard transport is available, otherwise show local help. |

With `transport-mode = "dashboard-ws"`, typing `/` shows live command suggestions from the dashboard catalog, `/resume` suggestions include loaded sessions with preview metadata, `/resume <query>` matches loaded session id, title, or preview text, and `/resume <stored-session-id>` can open id-shaped stored dashboard sessions. hermes-chat identifies dashboard requests as Hermes Desktop and creates dashboard sessions with the desktop source metadata so Hermes Agent returns the same rich markdown and event payloads used by Hermes Desktop. `/skin` argument suggestions use local Lumen theme presets, `/branch` creates a new dashboard session seeded from the latest user or assistant message, `/title <name>` renames through the dashboard `session.title` RPC, `/yolo` toggles session-scoped approval bypass through `config.set`, `/browser` manages the local gateway's browser CDP connection through `browser.manage`, `/handoff <platform>` requests and polls dashboard handoff state through `handoff.request` and `handoff.state`, `/profile <name>` validates dashboard profiles from `/api/profiles` and applies the selected profile to subsequent dashboard `session.create` calls, `/skin` applies Lumen theme presets locally, `/help` and `/commands` use the dashboard `commands.catalog` RPC, and other slash commands are executed through the Hermes dashboard gateway (`slash.exec` with `command.dispatch` fallback), so backend commands, skill commands, aliases, send directives, and prefill directives behave inline like Hermes Desktop. The fallback slash palette also lists Desktop backend commands such as `/agents`, `/background`, `/goal`, `/queue`, `/status`, `/stop`, `/tools`, `/usage`, and `/version`; `/status` and `/usage` follow Desktop semantics and render Hermes backend output inline, `/stop` stops background processes, and the dropdown Stop button cancels the active response. Prefill directives load the returned draft into the composer. Picker-owned and known no-desktop-surface commands are not sent as prompts; for example bare `/model` reports that the desktop model picker owns that action, `/reload-mcp` reports that the advanced command is not shown in the desktop palette, and `/model <name>` can still reach Hermes backend dispatch. API/SSE transports send unknown slash commands as normal prompts.

## General

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable the Hermes chat client module. |
| `endpoint-url` | string | `"http://127.0.0.1:8642"` | Hermes API server base URL. `/v1` suffix is accepted and normalized. |
| `api-key` | string | `"$HERMES_API_SERVER_KEY"` | Hermes API bearer token. `$HERMES_API_SERVER_KEY` is recommended. |
| `dashboard-token` | string | `"$HERMES_DESKTOP_REMOTE_TOKEN"` | Hermes Desktop dashboard session token. `$HERMES_DESKTOP_REMOTE_TOKEN` is recommended. |
| `model` | string | `"hermes-agent"` | Cosmetic model name sent to OpenAI-compatible endpoints. |
| `session-key` | string | `""` | Optional `X-Hermes-Session-Key` used by Hermes for server-side memory scoping. |
| `transport-mode` | [`HermesChatTransportMode`](/config/types#hermes-chat-transport-mode) | `"auto"` | Preferred API transport mode. |
| `local-history` | [`HermesChatLocalHistory`](/config/types#hermes-chat-local-history) | `"full"` | Local history persistence policy. `full` stores transcripts locally. |
| `history-limit` | u32 | `200` | Maximum number of messages kept in the local transcript. |
| `request-timeout-seconds` | u32 | `120` | Request timeout in seconds. |
| `show-tool-progress` | bool | `true` | Show Hermes tool progress rows in the transcript. |
| `show-runtime-warning` | bool | `true` | Show a warning that tools execute on the remote Hermes API server. |
| `icon-name` | string | `"ld-message-circle-symbolic"` | Icon for the bar button. |
| `border-show` | bool | `false` | Display border around button. |
| `icon-show` | bool | `true` | Display module icon. |
| `label-show` | bool | `false` | Display chat status label. |
| `label-max-length` | u32 | `32` | Max label characters before truncation with ellipsis. Set to 0 to disable. |

## Colors

| Field | Type | Default | Description |
|---|---|---|---|
| `border-color` | [`ColorValue`](/config/types#color-value) | `"border-accent"` | Border color token. |
| `icon-color` | [`ColorValue`](/config/types#color-value) | `"auto"` | Icon foreground color. Auto selects based on variant for contrast. |
| `icon-bg-color` | [`ColorValue`](/config/types#color-value) | `"accent"` | Icon container background color token. |
| `label-color` | [`ColorValue`](/config/types#color-value) | `"fg-default"` | Label text color token. |
| `button-bg-color` | [`ColorValue`](/config/types#color-value) | `"bg-surface-elevated"` | Button background color token. |

## Click actions

| Field | Type | Default | Description |
|---|---|---|---|
| `left-click` | [`ClickAction`](/config/types#click-action) | `"dropdown:hermes-chat"` | Action on left click. |
| `right-click` | [`ClickAction`](/config/types#click-action) | `""` | Action on right click. |
| `middle-click` | [`ClickAction`](/config/types#click-action) | `""` | Action on middle click. |
| `scroll-up` | [`ClickAction`](/config/types#click-action) | `""` | Action on scroll up. |
| `scroll-down` | [`ClickAction`](/config/types#click-action) | `""` | Action on scroll down. |

## Dropdown

| Field | Type | Default | Description |
|---|---|---|---|
| `dropdown-scale` | [`ScaleFactor`](/config/types#scale-factor) | `0.95` | Dropdown size scale, applied on top of the global UI scale. |

## Default configuration

```toml
[modules.hermes-chat]
enabled = false
endpoint-url = "http://127.0.0.1:8642"
api-key = "$HERMES_API_SERVER_KEY"
dashboard-token = "$HERMES_DESKTOP_REMOTE_TOKEN"
model = "hermes-agent"
session-key = ""
transport-mode = "auto"
local-history = "full"
history-limit = 200
request-timeout-seconds = 120
show-tool-progress = true
show-runtime-warning = true
dropdown-scale = 0.949999988079071
icon-name = "ld-message-circle-symbolic"
border-show = false
border-color = "border-accent"
icon-show = true
icon-color = "auto"
icon-bg-color = "accent"
label-show = false
label-color = "fg-default"
label-max-length = 32
button-bg-color = "bg-surface-elevated"
left-click = "dropdown:hermes-chat"
right-click = ""
middle-click = ""
scroll-up = ""
scroll-down = ""
```


</div>
