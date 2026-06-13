use std::{sync::Arc, time::Duration};

use chrono::Utc;
use lumen_core::Property;
use tokio::{
    sync::Notify,
    time::{interval, sleep},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::{
    error::{Error, error_chain},
    model::{ProviderEntry, ProviderKind, UsageSnapshot},
    provider::{CredentialPaths, create_provider},
    service::{ModelUsageErrorKind, ModelUsageStatus},
};

const MAX_RETRIES: u32 = 3;
const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) struct PollingConfig {
    pub poll_interval: Duration,
    pub providers: Vec<ProviderKind>,
    pub credential_paths: CredentialPaths,
    pub refresh: Arc<Notify>,
}

pub(crate) fn spawn(
    token: CancellationToken,
    usage: Property<Option<Arc<UsageSnapshot>>>,
    status: Property<ModelUsageStatus>,
    config: PollingConfig,
) {
    tokio::spawn(async move {
        let client = build_client();
        let mut ticker = interval(config.poll_interval);
        let mut first_tick = true;

        loop {
            tokio::select! {
                () = token.cancelled() => {
                    debug!("model usage polling stopped");
                    return;
                }
                () = config.refresh.notified() => {
                    status.set(ModelUsageStatus::Loading);
                    poll_once(&client, &token, &usage, &status, &config).await;
                    ticker.reset();
                }
                _ = ticker.tick() => {
                    if !first_tick && !usage.has_subscribers() {
                        continue;
                    }
                    first_tick = false;
                    poll_once(&client, &token, &usage, &status, &config).await;
                }
            }
        }
    });
}

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_default()
}

async fn poll_once(
    client: &reqwest::Client,
    token: &CancellationToken,
    usage: &Property<Option<Arc<UsageSnapshot>>>,
    status: &Property<ModelUsageStatus>,
    config: &PollingConfig,
) {
    let fetches = config
        .providers
        .iter()
        .map(|kind| fetch_provider(client, token, *kind, &config.credential_paths));
    let entries: Vec<ProviderEntry> = futures::future::join_all(fetches).await;

    let all_failed = entries.iter().all(|entry| entry.result.is_err());
    let first_error = entries
        .iter()
        .find_map(|entry| entry.result.as_ref().err().cloned());

    usage.set(Some(Arc::new(UsageSnapshot {
        updated_at: Utc::now(),
        providers: entries,
    })));

    match first_error {
        Some(kind) if all_failed => status.set(ModelUsageStatus::Error(kind)),
        _ => status.set(ModelUsageStatus::Loaded),
    }
}

async fn fetch_provider(
    client: &reqwest::Client,
    token: &CancellationToken,
    kind: ProviderKind,
    paths: &CredentialPaths,
) -> ProviderEntry {
    let provider = create_provider(kind, paths);

    for attempt in 1..=MAX_RETRIES {
        match provider.fetch(client).await {
            Ok(data) => {
                debug!(
                    provider = kind.id(),
                    windows = data.windows.len(),
                    "usage updated"
                );
                return ProviderEntry {
                    kind,
                    result: Ok(data),
                };
            }
            Err(err) if err.is_retryable() && attempt < MAX_RETRIES => {
                if wait_before_retry(token, &err, attempt).await.is_err() {
                    return ProviderEntry {
                        kind,
                        result: Err(ModelUsageErrorKind::from(&err)),
                    };
                }
            }
            Err(err) => {
                warn!(
                    provider = kind.id(),
                    error = %error_chain(&err),
                    "cannot fetch usage data"
                );
                return ProviderEntry {
                    kind,
                    result: Err(ModelUsageErrorKind::from(&err)),
                };
            }
        }
    }

    unreachable!("retry loop always returns")
}

async fn wait_before_retry(token: &CancellationToken, err: &Error, attempt: u32) -> Result<(), ()> {
    let delay = INITIAL_RETRY_DELAY * 2u32.saturating_pow(attempt - 1);
    debug!(
        error = %error_chain(err),
        attempt,
        delay_ms = delay.as_millis(),
        "usage fetch failed, retrying"
    );

    tokio::select! {
        () = token.cancelled() => Err(()),
        () = sleep(delay) => Ok(()),
    }
}
