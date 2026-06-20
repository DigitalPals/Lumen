use std::{fs, path::PathBuf, sync::RwLock};

use serde::{Deserialize, Serialize};

use crate::{HermesMessage, Result};

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredHistory {
    active_session_id: Option<String>,
    messages: Vec<HermesMessage>,
}

/// Simple local-only JSON transcript store.
#[derive(Debug)]
pub(crate) struct LocalHistoryStore {
    path: PathBuf,
    active_session_id: RwLock<Option<String>>,
}

impl LocalHistoryStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            active_session_id: RwLock::new(None),
        }
    }

    pub(crate) fn load(&self) -> Result<(Option<String>, Vec<HermesMessage>)> {
        if !self.path.is_file() {
            return Ok((None, Vec::new()));
        }
        let text = fs::read_to_string(&self.path)?;
        let history: StoredHistory = serde_json::from_str(&text)?;
        if let Ok(mut guard) = self.active_session_id.write() {
            *guard = history.active_session_id.clone();
        }
        Ok((history.active_session_id, history.messages))
    }

    pub(crate) fn set_active_session_id(&self, session_id: Option<String>) {
        if let Ok(mut guard) = self.active_session_id.write() {
            *guard = session_id;
        }
    }

    pub(crate) fn save(&self, messages: &[HermesMessage]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let active_session_id = self
            .active_session_id
            .read()
            .map(|guard| guard.clone())
            .unwrap_or(None);
        let history = StoredHistory {
            active_session_id,
            messages: messages.to_vec(),
        };
        fs::write(&self.path, serde_json::to_string_pretty(&history)?)?;
        Ok(())
    }
}
