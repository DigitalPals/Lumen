use gtk::{pango, prelude::*};
use lumen_network::vpn::{TailscaleStatus, VpnConnection, VpnKind, VpnProfile, VpnState};
use lumen_widgets::prelude::*;
use relm4::{gtk, prelude::*};
use zbus::zvariant::OwnedObjectPath;

use crate::i18n::t;

pub(super) enum VpnRowInit {
    Active(VpnConnection),
    Profile { profile: VpnProfile, active: bool },
    Tailscale(TailscaleStatus),
}

pub(super) struct VpnRow {
    name: String,
    detail: String,
    icon: &'static str,
    badge: String,
    action: Option<RowAction>,
}

#[derive(Clone)]
enum RowAction {
    ConnectProfile(OwnedObjectPath),
    DisconnectActive(OwnedObjectPath),
    TailscaleUp,
    TailscaleDown,
}

#[derive(Debug)]
pub(super) enum VpnRowInput {
    ActionClicked,
}

#[derive(Debug)]
pub(super) enum VpnRowOutput {
    ConnectProfile(OwnedObjectPath),
    DisconnectActive(OwnedObjectPath),
    TailscaleUp,
    TailscaleDown,
}

#[relm4::factory(pub(super))]
impl FactoryComponent for VpnRow {
    type Init = VpnRowInit;
    type Input = VpnRowInput;
    type Output = VpnRowOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::Box;

    view! {
        gtk::Box {
            add_css_class: "network-connection-card",

            gtk::Box {
                add_css_class: "network-connection-icon",
                set_hexpand: false,

                gtk::Image {
                    set_icon_name: Some(self.icon),
                    set_halign: gtk::Align::Center,
                    set_valign: gtk::Align::Center,
                },
            },

            gtk::Box {
                add_css_class: "network-connection-info",
                set_orientation: gtk::Orientation::Vertical,
                set_hexpand: true,

                gtk::Label {
                    add_css_class: "network-connection-name",
                    set_xalign: 0.0,
                    set_ellipsize: pango::EllipsizeMode::End,
                    set_max_width_chars: 1,
                    #[watch]
                    set_label: &self.name,
                },

                gtk::Label {
                    add_css_class: "network-connection-detail",
                    set_xalign: 0.0,
                    set_ellipsize: pango::EllipsizeMode::End,
                    set_max_width_chars: 1,
                    #[watch]
                    set_label: &self.detail,
                },
            },

            gtk::Box {
                add_css_class: "network-connection-actions",
                set_valign: gtk::Align::Center,

                #[template]
                SubtleBadge {
                    add_css_class: "network-connection-status",
                    #[watch]
                    set_label: &self.badge,
                },

                #[template]
                GhostButton {
                    #[watch]
                    set_visible: self.action.is_some(),
                    #[template_child]
                    label {
                        #[watch]
                        set_label: &self.action_label(),
                    },
                    connect_clicked => VpnRowInput::ActionClicked,
                },
            },
        }
    }

    fn init_model(init: Self::Init, _index: &Self::Index, _sender: FactorySender<Self>) -> Self {
        match init {
            VpnRowInit::Active(connection) => Self {
                icon: icon_for_kind(&connection.kind),
                detail: connection.kind.label().to_owned(),
                badge: state_label(connection.state),
                action: Some(RowAction::DisconnectActive(connection.active_path)),
                name: connection.name,
            },
            VpnRowInit::Profile { profile, active } => Self {
                icon: icon_for_kind(&profile.kind),
                detail: profile.kind.label().to_owned(),
                badge: if active {
                    t!("dropdown-vpn-connected")
                } else {
                    t!("dropdown-vpn-saved")
                },
                action: if active {
                    None
                } else {
                    Some(RowAction::ConnectProfile(profile.object_path))
                },
                name: profile.name,
            },
            VpnRowInit::Tailscale(status) => Self {
                icon: icon_for_kind(&VpnKind::Tailscale),
                detail: status
                    .tailnet
                    .clone()
                    .unwrap_or_else(|| String::from("Tailscale")),
                badge: if status.connected {
                    t!("dropdown-vpn-connected")
                } else {
                    status.backend_state.clone()
                },
                action: Some(if status.connected {
                    RowAction::TailscaleDown
                } else {
                    RowAction::TailscaleUp
                }),
                name: status
                    .self_name
                    .clone()
                    .unwrap_or_else(|| String::from("Tailscale")),
            },
        }
    }

    fn update(&mut self, msg: VpnRowInput, sender: FactorySender<Self>) {
        match msg {
            VpnRowInput::ActionClicked => {
                let Some(action) = self.action.clone() else {
                    return;
                };

                let output = match action {
                    RowAction::ConnectProfile(path) => VpnRowOutput::ConnectProfile(path),
                    RowAction::DisconnectActive(path) => VpnRowOutput::DisconnectActive(path),
                    RowAction::TailscaleUp => VpnRowOutput::TailscaleUp,
                    RowAction::TailscaleDown => VpnRowOutput::TailscaleDown,
                };

                let _ = sender.output(output);
            }
        }
    }
}

impl VpnRow {
    fn action_label(&self) -> String {
        match self.action {
            Some(RowAction::ConnectProfile(_) | RowAction::TailscaleUp) => {
                t!("dropdown-vpn-connect")
            }
            Some(RowAction::DisconnectActive(_) | RowAction::TailscaleDown) => {
                t!("dropdown-vpn-disconnect")
            }
            None => String::new(),
        }
    }
}

fn icon_for_kind(kind: &VpnKind) -> &'static str {
    match kind {
        VpnKind::WireGuard => "ld-shield-symbolic",
        VpnKind::Tailscale => "ld-shield-symbolic",
        VpnKind::NetworkManager
        | VpnKind::OpenVpn
        | VpnKind::OpenConnect
        | VpnKind::StrongSwan
        | VpnKind::Other(_) => "ld-shield-symbolic",
        _ => "ld-shield-symbolic",
    }
}

fn state_label(state: VpnState) -> String {
    match state {
        VpnState::Connected => t!("dropdown-vpn-connected"),
        VpnState::Connecting => t!("dropdown-vpn-connecting"),
        VpnState::NeedsAuth => t!("dropdown-vpn-needs-auth"),
        VpnState::Disconnecting => t!("dropdown-vpn-disconnecting"),
        VpnState::Failed => t!("dropdown-vpn-failed"),
        VpnState::Disconnected => t!("dropdown-vpn-disconnected"),
        VpnState::Unknown => t!("dropdown-vpn-unknown"),
    }
}
