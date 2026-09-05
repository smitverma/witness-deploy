use std::sync::Arc;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{
    error::{Result, WitnessError},
    event_bus::{Event, EventBus, ScopeEvent},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeEntry {
    pub id: i64,
    pub pattern: String,
    pub is_regex: bool,
    pub include_subdomains: bool,
    pub is_in_scope: bool,
}

/// Maximum accepted scope pattern length. Prevents ReDoS-scale regexes and
/// accidental megabyte pastes from degrading matching.
pub const MAX_PATTERN_LEN: usize = 512;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSnapshot {
    pub entries: Vec<ScopeEntry>,
}

#[derive(Clone, Default)]
pub struct ScopeManager {
    state: Arc<RwLock<ScopeSnapshot>>,
}

impl ScopeManager {
    pub async fn replace(&self, snapshot: ScopeSnapshot) {
        *self.state.write().await = snapshot;
    }

    pub async fn snapshot(&self) -> ScopeSnapshot {
        self.state.read().await.clone()
    }

    /// True when neither in-scope nor out-of-scope rules exist. In that state
    /// every host matches the scope, so scope-dependent toggles cannot change
    /// filtering behavior.
    pub async fn is_empty(&self) -> bool {
        self.state.read().await.entries.is_empty()
    }

    pub async fn add(&self, entry: ScopeEntry, bus: &EventBus) -> Result<()> {
        validate(&entry.pattern, entry.is_regex)?;
        let mut state = self.state.write().await;
        if state.entries.iter().any(|existing| existing.id == entry.id) {
            return Err(WitnessError::Other(anyhow::anyhow!(
                "scope entry id already exists"
            )));
        }
        state.entries.push(entry);
        bus.publish(Event::Scope(ScopeEvent::Changed));
        Ok(())
    }

    pub async fn remove(&self, id: i64, bus: &EventBus) {
        self.state
            .write()
            .await
            .entries
            .retain(|entry| entry.id != id);
        bus.publish(Event::Scope(ScopeEvent::Changed));
    }

    pub async fn update(&self, entry: ScopeEntry, bus: &EventBus) -> Result<()> {
        validate(&entry.pattern, entry.is_regex)?;
        let mut state = self.state.write().await;
        let existing = state
            .entries
            .iter_mut()
            .find(|existing| existing.id == entry.id)
            .ok_or_else(|| WitnessError::Other(anyhow::anyhow!("scope entry was not found")))?;
        *existing = entry;
        bus.publish(Event::Scope(ScopeEvent::Changed));
        Ok(())
    }

    pub async fn is_in_scope(&self, host: &str) -> bool {
        let state = self.state.read().await;
        // A new project intentionally starts with an allow-all scope. Adding an
        // in-scope rule turns the list into an allow-list; out-of-scope rules
        // always remain exclusions, including while the default is allow-all.
        let has_in_scope_rules = state.entries.iter().any(|entry| entry.is_in_scope);
        let in_scope = state
            .entries
            .iter()
            .filter(|entry| entry.is_in_scope)
            .any(|entry| matches(entry, host));
        let out_of_scope = state
            .entries
            .iter()
            .filter(|entry| !entry.is_in_scope)
            .any(|entry| matches(entry, host));
        (!has_in_scope_rules || in_scope) && !out_of_scope
    }
}

fn matches(entry: &ScopeEntry, host: &str) -> bool {
    if entry.is_regex {
        return Regex::new(&entry.pattern).is_ok_and(|regex| regex.is_match(host));
    }

    let host = host.trim_end_matches('.');
    let pattern = entry.pattern.trim_end_matches('.');
    host.eq_ignore_ascii_case(pattern)
        || (entry.include_subdomains
            && host.len() > pattern.len()
            && host.ends_with(pattern)
            && host.as_bytes().get(host.len() - pattern.len() - 1) == Some(&b'.'))
}

pub fn validate(pattern: &str, is_regex: bool) -> Result<()> {
    if pattern.trim().is_empty() {
        return Err(WitnessError::Other(anyhow::anyhow!(
            "scope pattern cannot be empty"
        )));
    }
    if pattern.len() > MAX_PATTERN_LEN {
        return Err(WitnessError::Other(anyhow::anyhow!(
            "scope pattern exceeds 512 characters"
        )));
    }
    if is_regex {
        Regex::new(pattern).map_err(|error| WitnessError::Other(error.into()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::EventBus;

    #[tokio::test]
    async fn matches_domains_and_regular_expressions() {
        let scope = ScopeManager::default();
        let bus = EventBus::new(8);
        scope
            .add(
                ScopeEntry {
                    id: 1,
                    pattern: "example.test".into(),
                    is_regex: false,
                    include_subdomains: true,
                    is_in_scope: true,
                },
                &bus,
            )
            .await
            .unwrap();
        scope
            .add(
                ScopeEntry {
                    id: 2,
                    pattern: r"^api\d+\.internal$".into(),
                    is_regex: true,
                    include_subdomains: false,
                    is_in_scope: true,
                },
                &bus,
            )
            .await
            .unwrap();
        assert!(scope.is_in_scope("sub.example.test").await);
        assert!(scope.is_in_scope("api2.internal").await);
        assert!(!scope.is_in_scope("outside.test").await);
    }

    #[tokio::test]
    async fn an_empty_scope_allows_every_host() {
        let scope = ScopeManager::default();
        assert!(scope.is_in_scope("example.test").await);
        assert!(scope.is_in_scope("api.example.test").await);
    }

    #[tokio::test]
    async fn is_empty_reflects_rule_presence() {
        let scope = ScopeManager::default();
        assert!(scope.is_empty().await);

        let bus = EventBus::new(8);
        scope
            .add(
                ScopeEntry {
                    id: 1,
                    pattern: "telemetry.example.test".into(),
                    is_regex: false,
                    include_subdomains: false,
                    is_in_scope: false,
                },
                &bus,
            )
            .await
            .unwrap();
        assert!(!scope.is_empty().await);
    }

    #[tokio::test]
    async fn out_of_scope_rules_exclude_hosts_from_the_default_allow_all_scope() {
        let scope = ScopeManager::default();
        let bus = EventBus::new(8);
        scope
            .add(
                ScopeEntry {
                    id: 1,
                    pattern: "telemetry.example.test".into(),
                    is_regex: false,
                    include_subdomains: true,
                    is_in_scope: false,
                },
                &bus,
            )
            .await
            .unwrap();

        assert!(scope.is_in_scope("app.example.test").await);
        assert!(!scope.is_in_scope("telemetry.example.test").await);
        assert!(!scope.is_in_scope("api.telemetry.example.test").await);
    }

    #[tokio::test]
    async fn out_of_scope_entries_override_in_scope_entries() {
        let scope = ScopeManager::default();
        let bus = EventBus::new(8);
        for entry in [
            ScopeEntry {
                id: 1,
                pattern: "example.test".into(),
                is_regex: false,
                include_subdomains: true,
                is_in_scope: true,
            },
            ScopeEntry {
                id: 2,
                pattern: "test.example.test".into(),
                is_regex: false,
                include_subdomains: true,
                is_in_scope: false,
            },
        ] {
            scope.add(entry, &bus).await.unwrap();
        }
        assert!(scope.is_in_scope("api.example.test").await);
        assert!(!scope.is_in_scope("test.example.test").await);
        assert!(!scope.is_in_scope("api.test.example.test").await);
    }

    #[tokio::test]
    async fn exact_domain_entries_do_not_match_subdomains() {
        let scope = ScopeManager::default();
        let bus = EventBus::new(8);
        scope
            .add(
                ScopeEntry {
                    id: 1,
                    pattern: "example.test".into(),
                    is_regex: false,
                    include_subdomains: false,
                    is_in_scope: true,
                },
                &bus,
            )
            .await
            .unwrap();
        assert!(scope.is_in_scope("example.test").await);
        assert!(!scope.is_in_scope("api.example.test").await);
    }
}
