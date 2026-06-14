use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use tokio::time;
use tracing::{debug, warn};

use super::VpnManager;
use crate::{error::Error, proxy::manager::NetworkManagerProxy};

impl VpnManager {
    pub(super) async fn start_monitoring(self: Arc<Self>) -> Result<(), Error> {
        let nm_proxy = NetworkManagerProxy::new(&self.zbus_connection).await?;
        let mut active_connections_changed = nm_proxy.receive_active_connections_changed().await;
        let mut profile_changes = self.settings.connections.watch();
        let cancellation_token = self.cancellation_token.clone();
        let manager = self.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(5));
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        debug!("VpnManager monitoring cancelled");
                        return;
                    }
                    Some(_) = profile_changes.next() => {
                        manager.refresh_profiles();
                    }
                    Some(change) = active_connections_changed.next() => {
                        if change.get().await.is_err() {
                            warn!("cannot read active VPN connection change");
                        }
                        manager.refresh_active().await;
                    }
                    _ = interval.tick() => {
                        manager.refresh_active().await;
                        manager.refresh_tailscale().await;
                    }
                }
            }
        });

        Ok(())
    }
}
