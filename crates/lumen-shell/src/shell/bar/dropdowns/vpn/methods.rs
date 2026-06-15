use relm4::{factory::FactoryVecDeque, prelude::*};
use tracing::warn;

use super::{
    VpnDropdown,
    messages::{VpnDropdownCmd, VpnDropdownMsg},
    row::{VpnRowInit, VpnRowOutput},
};

pub(super) fn forward_row_output(output: VpnRowOutput) -> VpnDropdownMsg {
    match output {
        VpnRowOutput::ConnectProfile(path) => VpnDropdownMsg::ConnectProfile(path),
        VpnRowOutput::DisconnectActive(path) => VpnDropdownMsg::DisconnectActive(path),
        VpnRowOutput::TailscaleUp => VpnDropdownMsg::TailscaleUp,
        VpnRowOutput::TailscaleDown => VpnDropdownMsg::TailscaleDown,
    }
}

impl VpnDropdown {
    pub(super) fn rebuild_rows(&mut self) {
        let active = self.network.vpn.active.get();
        let profiles = self.network.vpn.profiles.get();
        let tailscale = self.network.vpn.tailscale.get();
        let config = &self.config.config().modules.vpn;
        let active_profile_paths = active
            .iter()
            .map(|connection| connection.profile_path.clone())
            .collect::<Vec<_>>();

        self.has_profiles = !profiles.is_empty() || tailscale.is_some();
        self.has_active = !active.is_empty()
            || tailscale
                .as_ref()
                .is_some_and(|status| status.connected || status.backend_state == "Starting");

        replace_rows(
            &mut self.active_list,
            active
                .into_iter()
                .map(|connection| VpnRowInit::Active {
                    icon: super::row::active_icon_for_state(config, connection.state),
                    connection,
                })
                .chain(tailscale.clone().into_iter().filter_map(|status| {
                    if status.connected || status.backend_state == "Starting" {
                        Some(VpnRowInit::Tailscale {
                            icon: super::row::tailscale_active_icon(config, &status),
                            active: true,
                            status,
                        })
                    } else {
                        None
                    }
                })),
        );

        replace_rows(
            &mut self.profile_list,
            profiles
                .into_iter()
                .map(|profile| VpnRowInit::Profile {
                    active: active_profile_paths
                        .iter()
                        .any(|path| path == &profile.object_path),
                    profile,
                })
                .chain(tailscale.into_iter().filter_map(|status| {
                    if status.connected || status.backend_state == "Starting" {
                        None
                    } else {
                        Some(VpnRowInit::Tailscale {
                            icon: super::row::tailscale_active_icon(config, &status),
                            active: false,
                            status,
                        })
                    }
                })),
        );
    }

    pub(super) fn connect_profile(
        &self,
        path: zbus::zvariant::OwnedObjectPath,
        sender: &ComponentSender<Self>,
    ) {
        let vpn = self.network.vpn.clone();
        sender.command(move |out, _shutdown| async move {
            if let Err(err) = vpn.connect_profile(&path).await {
                warn!(error = %err, "vpn activation failed");
                let _ = out.send(VpnDropdownCmd::OperationFailed(err.to_string()));
            }
            vpn.refresh_active().await;
        });
    }

    pub(super) fn disconnect_active(
        &self,
        path: zbus::zvariant::OwnedObjectPath,
        sender: &ComponentSender<Self>,
    ) {
        let vpn = self.network.vpn.clone();
        sender.command(move |out, _shutdown| async move {
            if let Err(err) = vpn.disconnect_active(&path).await {
                warn!(error = %err, "vpn deactivation failed");
                let _ = out.send(VpnDropdownCmd::OperationFailed(err.to_string()));
            }
            vpn.refresh_active().await;
        });
    }

    pub(super) fn tailscale_up(&self, sender: &ComponentSender<Self>) {
        let vpn = self.network.vpn.clone();
        sender.command(move |out, _shutdown| async move {
            if let Err(err) = vpn.tailscale_up().await {
                warn!(error = %err, "tailscale up failed");
                let _ = out.send(VpnDropdownCmd::OperationFailed(err.to_string()));
            }
            vpn.refresh_tailscale().await;
        });
    }

    pub(super) fn tailscale_down(&self, sender: &ComponentSender<Self>) {
        let vpn = self.network.vpn.clone();
        sender.command(move |out, _shutdown| async move {
            if let Err(err) = vpn.tailscale_down().await {
                warn!(error = %err, "tailscale down failed");
                let _ = out.send(VpnDropdownCmd::OperationFailed(err.to_string()));
            }
            vpn.refresh_tailscale().await;
        });
    }
}

fn replace_rows(
    list: &mut FactoryVecDeque<super::row::VpnRow>,
    rows: impl IntoIterator<Item = VpnRowInit>,
) {
    let mut guard = list.guard();
    guard.clear();

    for row in rows {
        guard.push_back(row);
    }
}
