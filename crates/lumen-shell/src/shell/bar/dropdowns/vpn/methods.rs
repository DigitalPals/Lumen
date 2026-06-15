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
        VpnRowOutput::OpenTailscaleAdmin => VpnDropdownMsg::OpenTailscaleAdmin,
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
                let _ = out.send(VpnDropdownCmd::OperationFailed(tailscale_error_message(
                    &err,
                )));
            }
            vpn.refresh_tailscale().await;
        });
    }

    pub(super) fn tailscale_down(&self, sender: &ComponentSender<Self>) {
        let vpn = self.network.vpn.clone();
        sender.command(move |out, _shutdown| async move {
            if let Err(err) = vpn.tailscale_down().await {
                warn!(error = %err, "tailscale down failed");
                let _ = out.send(VpnDropdownCmd::OperationFailed(tailscale_error_message(
                    &err,
                )));
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

fn tailscale_error_message(err: &dyn std::error::Error) -> String {
    let mut current = Some(err);

    while let Some(error) = current {
        let message = error.to_string();
        if tailscale_permission_denied(&message) {
            return String::from(
                "Tailscale needs permission to connect or disconnect from Lumen. Run `sudo \
                 tailscale set --operator=$USER` once in a terminal, then try again.",
            );
        }
        current = error.source();
    }

    err.to_string()
}

fn tailscale_permission_denied(message: &str) -> bool {
    message.contains("prefs write access denied")
        || message.contains("Use 'sudo tailscale")
        || message.contains("sudo tailscale set --operator")
}

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;

    use super::*;

    #[derive(Debug)]
    struct ErrorWithSource {
        source: Box<dyn StdError + Send + Sync>,
    }

    impl std::fmt::Display for ErrorWithSource {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "cannot stop tailscale")
        }
    }

    impl StdError for ErrorWithSource {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            Some(self.source.as_ref())
        }
    }

    #[derive(Debug)]
    struct MessageError(&'static str);

    impl std::fmt::Display for MessageError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl StdError for MessageError {}

    #[test]
    fn tailscale_error_message_explains_operator_permission_fix() {
        let err = ErrorWithSource {
            source: Box::new(MessageError(
                "Access denied: prefs write access denied\n\nUse 'sudo tailscale down'.",
            )),
        };

        let message = tailscale_error_message(&err);

        assert!(message.contains("sudo tailscale set --operator=$USER"));
        assert!(message.contains("then try again"));
    }

    #[test]
    fn tailscale_error_message_preserves_unrecognized_errors() {
        let err = MessageError("cannot stop tailscale");

        assert_eq!(tailscale_error_message(&err), "cannot stop tailscale");
    }
}
