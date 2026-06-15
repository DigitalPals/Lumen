use gtk::{pango, prelude::*};
use lumen_config::schemas::modules::VpnConfig;
use lumen_network::vpn::{TailscaleStatus, VpnConnection, VpnKind, VpnProfile, VpnState};
use lumen_widgets::prelude::*;
use relm4::{gtk, prelude::*};
use zbus::zvariant::OwnedObjectPath;

use crate::i18n::t;

pub(super) enum VpnRowInit {
    Active {
        connection: VpnConnection,
        icon: String,
    },
    Profile {
        profile: VpnProfile,
        active: bool,
    },
    Tailscale {
        status: TailscaleStatus,
        active: bool,
        icon: String,
    },
}

pub(super) struct VpnRow {
    name: String,
    detail: String,
    icon: String,
    badge: Option<String>,
    active_connected: bool,
    action: Option<RowAction>,
    open_admin_on_click: bool,
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
    OpenAdminClicked,
}

#[derive(Debug)]
pub(super) enum VpnRowOutput {
    ConnectProfile(OwnedObjectPath),
    DisconnectActive(OwnedObjectPath),
    OpenTailscaleAdmin,
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
            set_orientation: gtk::Orientation::Horizontal,

            #[name = "row_body"]
            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_hexpand: true,

                gtk::Box {
                    #[watch]
                    set_css_classes: &self.icon_classes(),
                    set_hexpand: false,

                    gtk::Image {
                        #[watch]
                        set_icon_name: Some(&self.icon),
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
            },

            gtk::Box {
                add_css_class: "network-connection-actions",
                set_valign: gtk::Align::Center,

                #[template]
                SubtleBadge {
                    add_css_class: "network-connection-status",
                    #[watch]
                    set_visible: self.badge.is_some(),
                    #[watch]
                    set_label: self.badge.as_deref().unwrap_or(""),
                },

                #[template]
                GhostButton {
                    #[watch]
                    set_visible: self.action.is_some() && !self.action_icon_only(),
                    #[template_child]
                    label {
                        #[watch]
                        set_label: &self.action_label(),
                    },
                    connect_clicked => VpnRowInput::ActionClicked,
                },

                #[template]
                GhostIconButton {
                    add_css_class: "network-action-disconnect",
                    #[watch]
                    set_visible: self.action.is_some() && self.action_icon_only(),
                    set_icon_name: "ld-unplug-symbolic",
                    #[watch]
                    set_tooltip_text: Some(&self.action_label()),
                    connect_clicked => VpnRowInput::ActionClicked,
                },
            },
        }
    }

    fn init_model(init: Self::Init, _index: &Self::Index, _sender: FactorySender<Self>) -> Self {
        match init {
            VpnRowInit::Active { connection, icon } => Self {
                active_connected: connection.state == VpnState::Connected,
                icon,
                detail: connection.kind.label().to_owned(),
                badge: None,
                action: Some(RowAction::DisconnectActive(connection.active_path)),
                open_admin_on_click: false,
                name: connection.name,
            },
            VpnRowInit::Profile { profile, active } => Self {
                icon: icon_for_kind(&profile.kind).to_owned(),
                detail: profile.kind.label().to_owned(),
                badge: Some(if active {
                    t!("dropdown-vpn-connected")
                } else {
                    t!("dropdown-vpn-saved")
                }),
                active_connected: false,
                action: if active {
                    None
                } else {
                    Some(RowAction::ConnectProfile(profile.object_path))
                },
                open_admin_on_click: false,
                name: profile.name,
            },
            VpnRowInit::Tailscale {
                status,
                active,
                icon,
            } => Self {
                icon: if active {
                    icon
                } else {
                    icon_for_kind(&VpnKind::Tailscale).to_owned()
                },
                detail: status
                    .tailnet
                    .clone()
                    .unwrap_or_else(|| String::from("Tailscale")),
                badge: if active {
                    None
                } else {
                    Some(if status.connected {
                        t!("dropdown-vpn-connected")
                    } else {
                        status.backend_state.clone()
                    })
                },
                active_connected: active && status.connected,
                action: Some(if status.connected {
                    RowAction::TailscaleDown
                } else {
                    RowAction::TailscaleUp
                }),
                open_admin_on_click: true,
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
            VpnRowInput::OpenAdminClicked => {
                if self.open_admin_on_click {
                    let _ = sender.output(VpnRowOutput::OpenTailscaleAdmin);
                }
            }
        }
    }

    fn init_widgets(
        &mut self,
        _index: &Self::Index,
        _root: Self::Root,
        _returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        let widgets = view_output!();

        if self.open_admin_on_click {
            let click = gtk::GestureClick::new();
            let click_sender = sender.input_sender().clone();
            click.connect_released(move |gesture, _, _, _| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                click_sender.emit(VpnRowInput::OpenAdminClicked);
            });
            widgets.row_body.add_controller(click);
            widgets.row_body.set_cursor_from_name(Some("pointer"));
        }

        widgets
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

    fn action_icon_only(&self) -> bool {
        matches!(
            self.action,
            Some(RowAction::DisconnectActive(_) | RowAction::TailscaleDown)
        )
    }

    fn icon_classes(&self) -> Vec<&'static str> {
        let mut classes = vec!["network-connection-icon"];
        if self.active_connected {
            classes.push("vpn-connected");
        }
        classes
    }
}

pub(super) fn active_icon_for_state(config: &VpnConfig, state: VpnState) -> String {
    match state {
        VpnState::Connected => config.connected_icon.get().clone(),
        VpnState::Connecting => config.connecting_icon.get().clone(),
        _ => config.disconnected_icon.get().clone(),
    }
}

pub(super) fn tailscale_active_icon(config: &VpnConfig, status: &TailscaleStatus) -> String {
    if status.backend_state == "Starting" {
        config.connecting_icon.get().clone()
    } else if status.connected {
        config.connected_icon.get().clone()
    } else {
        config.disconnected_icon.get().clone()
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
