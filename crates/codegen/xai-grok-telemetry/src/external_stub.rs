//! No-export implementation used by the standalone serve runtime.
//!
//! The runtime keeps the original telemetry call sites and configuration
//! types, but deliberately has no collector, queue, or transport backend.

#[path = "external/config.rs"]
pub mod config;
#[allow(dead_code)]
#[path = "external/schema.rs"]
pub mod schema;
#[path = "external/truncate.rs"]
pub mod truncate;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

pub use config::{ContentGates, ExternalOtelConfig, ExternalOtelFileConfig};

#[derive(Debug, Clone, Default)]
pub struct IdentityAttrs {
    pub user_id: Option<String>,
    pub organization_id: Option<String>,
    pub team_id: Option<String>,
    pub deployment_id: Option<String>,
}

impl IdentityAttrs {
    pub fn from_snapshot(snapshot: &xai_grok_auth::CredentialSnapshot) -> Self {
        Self {
            user_id: snapshot.user_id.clone(),
            organization_id: snapshot.organization_id.clone(),
            team_id: snapshot.team_id.clone(),
            deployment_id: snapshot.deployment_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExternalOtelRemotePolicy {
    pub force_disable: bool,
    pub lock_content_gates: bool,
}

pub struct ExternalTelemetry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportHealthSnapshot {
    pub records_dropped: u64,
    pub metric_exports_dropped: u64,
    pub export_failures: u64,
    pub export_successes: u64,
}

const DEFAULT_SETTINGS_GATE_MAX_WAIT: Duration = Duration::from_secs(30);
static SETTINGS_RESOLVED: AtomicBool = AtomicBool::new(true);
static SETTINGS_GATE_MAX_WAIT_MS: AtomicU64 =
    AtomicU64::new(DEFAULT_SETTINGS_GATE_MAX_WAIT.as_millis() as u64);

pub fn init(_cfg: Option<ExternalOtelConfig>) {}

pub fn set_settings_gate_max_wait(max_wait: Duration) {
    SETTINGS_GATE_MAX_WAIT_MS.store(
        u64::try_from(max_wait.as_millis()).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
}

pub fn settings_gate_max_wait() -> Duration {
    Duration::from_millis(SETTINGS_GATE_MAX_WAIT_MS.load(Ordering::Relaxed))
}

pub fn suppress_external_otel_until_settings() {
    SETTINGS_RESOLVED.store(false, Ordering::Release);
}

pub fn mark_external_otel_settings_resolved() {
    SETTINGS_RESOLVED.store(true, Ordering::Release);
}

pub fn is_settings_gate_open() -> bool {
    SETTINGS_RESOLVED.load(Ordering::Acquire)
}

pub fn is_active() -> bool {
    false
}

pub fn emit<T: crate::events::TelemetryEvent>(_data: &T) {}

pub fn set_identity(_attrs: IdentityAttrs) {}

pub fn apply_remote_policy(_policy: ExternalOtelRemotePolicy) {}

pub fn flush() {}

pub fn shutdown() {}

pub fn export_health() -> Option<ExportHealthSnapshot> {
    None
}
