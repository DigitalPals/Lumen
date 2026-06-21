### Lumen Configuration - Hermes Chat Module

## Hermes Chat Module Configuration

settings-modules-hermes-chat-enabled = Enabled
    .description = Enable the native Hermes Agent chat dropdown

settings-modules-hermes-chat-endpoint-url = Endpoint URL
    .description = Hermes API server base URL. A trailing /v1 is accepted and normalized

settings-modules-hermes-chat-api-key = API Key
    .description = Bearer token for Hermes API server. Referencing $HERMES_API_SERVER_KEY in a .env file is recommended

settings-modules-hermes-chat-dashboard-token = Dashboard Token
    .description = Session token for Hermes Desktop dashboard WebSocket. Referencing $HERMES_DESKTOP_REMOTE_TOKEN in a .env file is recommended

settings-modules-hermes-chat-model = Model
    .description = Cosmetic model name sent to OpenAI-compatible Hermes endpoints

settings-modules-hermes-chat-session-key = Session Key
    .description = Optional X-Hermes-Session-Key for Hermes server-side memory scoping

settings-modules-hermes-chat-transport-mode = Transport Mode
    .description = Preferred Hermes API transport mode

settings-modules-hermes-chat-local-history = Local History
    .description = Whether Lumen stores full transcript history locally

## HermesChatTransportMode variants
enum-hermes-chat-transport-mode-auto = Auto
enum-hermes-chat-transport-mode-sessions = Sessions
enum-hermes-chat-transport-mode-runs = Runs
enum-hermes-chat-transport-mode-chat-completions = Chat Completions
enum-hermes-chat-transport-mode-dashboard-ws = Dashboard WebSocket

## HermesChatLocalHistory variants
enum-hermes-chat-local-history-disabled = Disabled
enum-hermes-chat-local-history-full = Full

settings-modules-hermes-chat-history-limit = History Limit
    .description = Maximum number of local transcript messages to keep

settings-modules-hermes-chat-request-timeout-seconds = Request Timeout
    .description = HTTP request timeout in seconds

settings-modules-hermes-chat-show-tool-progress = Show Tool Progress
    .description = Display Hermes tool progress rows in the chat transcript

settings-modules-hermes-chat-show-runtime-warning = Runtime Warning
    .description = Show that tools run on the remote Hermes API server host

settings-modules-hermes-chat-dropdown-scale = Dropdown Size
    .description = Dropdown size scale, applied on top of the global UI scale

settings-modules-hermes-chat-icon-name = Icon
    .description = Icon for the bar button

settings-modules-hermes-chat-border-show = Show Border
    .description = Display border around button

settings-modules-hermes-chat-border-color = Border Color
    .description = Border color token

settings-modules-hermes-chat-icon-show = Show Icon
    .description = Display module icon

settings-modules-hermes-chat-icon-color = Icon Color
    .description = Icon foreground color

settings-modules-hermes-chat-icon-bg-color = Icon Background
    .description = Icon container background color token

settings-modules-hermes-chat-label-show = Show Label
    .description = Display chat status label

settings-modules-hermes-chat-label-color = Label Color
    .description = Label text color token

settings-modules-hermes-chat-label-max-length = Label Max Length
    .description = Max label characters before truncation with ellipsis. Set to 0 to disable

settings-modules-hermes-chat-button-bg-color = Button Background
    .description = Button background color token

settings-modules-hermes-chat-left-click = Left Click
    .description = Action on left click

settings-modules-hermes-chat-right-click = Right Click
    .description = Action on right click

settings-modules-hermes-chat-middle-click = Middle Click
    .description = Action on middle click

settings-modules-hermes-chat-scroll-up = Scroll Up
    .description = Action on scroll up

settings-modules-hermes-chat-scroll-down = Scroll Down
    .description = Action on scroll down
