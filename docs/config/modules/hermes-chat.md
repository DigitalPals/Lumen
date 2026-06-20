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

Hermes tools run on the **Hermes API server host**, not on the Lumen desktop client. Start Hermes with `API_SERVER_ENABLED=true`, set `API_SERVER_KEY=[REDACTED]`, and point `api-key` at a `$VAR_NAME` such as `$HERMES_API_SERVER_KEY`.

## General

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable the Hermes chat client module. |
| `endpoint-url` | string | `"http://127.0.0.1:8642"` | Hermes API server base URL. `/v1` suffix is accepted and normalized. |
| `api-key` | string | `"$HERMES_API_SERVER_KEY"` | Hermes API bearer token. `$HERMES_API_SERVER_KEY` is recommended. |
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
