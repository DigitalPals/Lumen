---
title: vpn
outline: [2, 3]
---

# vpn

<div v-pre>

VPN connection status with a dropdown for NetworkManager VPN profiles, WireGuard profiles, and Tailscale.

Add it to your layout with `vpn`:

```toml
[[bar.layout]]
monitor = "*"
right = ["vpn"]
```

## General

| Field | Type | Default | Description |
|---|---|---|---|
| `connected-icon` | string | `"ld-shield-check-symbolic"` | Icon when a VPN is connected. |
| `connecting-icon` | string | `"ld-refresh-cw-symbolic"` | Icon when a VPN is connecting. |
| `disconnected-icon` | string | `"ld-shield-symbolic"` | Icon when no VPN is connected. |
| `border-show` | bool | `false` | Display border around button. |
| `icon-show` | bool | `true` | Display module icon. |
| `label-show` | bool | `true` | Display VPN label. |
| `tailscale-label` | string | `"service-name"` | Label shown when Tailscale is connected: `"service-name"`, `"hostname"`, or `"status"`. |
| `label-max-length` | u32 | `18` | Max label characters before truncation with ellipsis. Set to 0 to disable. |

## Colors

| Field | Type | Default | Description |
|---|---|---|---|
| `border-color` | [`ColorValue`](/config/types#color-value) | `"accent"` | Border color token. |
| `icon-color` | [`ColorValue`](/config/types#color-value) | `"auto"` | Icon foreground color. Auto selects based on variant for contrast. |
| `icon-bg-color` | [`ColorValue`](/config/types#color-value) | `"accent"` | Icon container background color token. |
| `label-color` | [`ColorValue`](/config/types#color-value) | `"accent"` | Label text color token. |
| `button-bg-color` | [`ColorValue`](/config/types#color-value) | `"bg-surface-elevated"` | Button background color token. |

## Click actions

| Field | Type | Default | Description |
|---|---|---|---|
| `left-click` | [`ClickAction`](/config/types#click-action) | `"dropdown:vpn"` | Action on left click. |
| `right-click` | [`ClickAction`](/config/types#click-action) | `""` | Action on right click. |
| `middle-click` | [`ClickAction`](/config/types#click-action) | `""` | Action on middle click. |
| `scroll-up` | [`ClickAction`](/config/types#click-action) | `""` | Action on scroll up. |
| `scroll-down` | [`ClickAction`](/config/types#click-action) | `""` | Action on scroll down. |

## Default configuration

```toml
[modules.vpn]
connected-icon = "ld-shield-check-symbolic"
connecting-icon = "ld-refresh-cw-symbolic"
disconnected-icon = "ld-shield-symbolic"
border-show = false
border-color = "accent"
icon-show = true
icon-color = "auto"
icon-bg-color = "accent"
label-show = true
tailscale-label = "service-name"
label-color = "accent"
label-max-length = 18
button-bg-color = "bg-surface-elevated"
left-click = "dropdown:vpn"
right-click = ""
middle-click = ""
scroll-up = ""
scroll-down = ""
```

</div>
