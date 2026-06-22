//! Pure domain types and invariants for ScreenSearch.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Stable identifier for a captured frame.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct CaptureId(pub Uuid);

impl CaptureId {
    /// Creates a time-sortable identifier.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for CaptureId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CaptureId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identifier for a durable analysis job.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

impl JobId {
    /// Creates a time-sortable identifier.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identifier for an indexed text chunk.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ChunkId(pub Uuid);

impl ChunkId {
    /// Creates a time-sortable identifier.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ChunkId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ChunkId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identifier for one automation approval and its optional run.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AutomationRunId(pub Uuid);

impl AutomationRunId {
    /// Creates a time-sortable automation run identifier.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for AutomationRunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for AutomationRunId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Exact foreground identity approved for one automation plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationTarget {
    /// Operating-system process identifier.
    pub process_id: u32,
    /// Native window handle represented without pointer semantics.
    pub window_handle: u64,
    /// Bounded executable file name, without a directory.
    pub executable_name: String,
    /// User-visible title used only for review and live IPC.
    pub display_title: String,
}

impl AutomationTarget {
    /// Validates a complete, bounded target identity.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.process_id == 0 || self.window_handle == 0 {
            return Err(DomainError::InvalidAutomation(
                "target process and window handle must be non-zero".to_owned(),
            ));
        }
        validate_automation_text("target executable", &self.executable_name, 1, 260)?;
        if self.executable_name.contains(['/', '\\']) {
            return Err(DomainError::InvalidAutomation(
                "target executable must be a file name, not a path".to_owned(),
            ));
        }
        validate_automation_text("target display title", &self.display_title, 1, 512)
    }
}

/// Modifier accepted by an explicit keyboard fallback action.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyModifier {
    /// Control key.
    Control,
    /// Alternate key.
    Alt,
    /// Shift key.
    Shift,
}

/// Bounded non-modifier key accepted by an explicit chord action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationKey {
    /// A key.
    A,
    /// B key.
    B,
    /// C key.
    C,
    /// D key.
    D,
    /// E key.
    E,
    /// F key.
    F,
    /// G key.
    G,
    /// H key.
    H,
    /// I key.
    I,
    /// J key.
    J,
    /// K key.
    K,
    /// L key.
    L,
    /// M key.
    M,
    /// N key.
    N,
    /// O key.
    O,
    /// P key.
    P,
    /// Q key.
    Q,
    /// R key.
    R,
    /// S key.
    S,
    /// T key.
    T,
    /// U key.
    U,
    /// V key.
    V,
    /// W key.
    W,
    /// X key.
    X,
    /// Y key.
    Y,
    /// Z key.
    Z,
    /// Digit zero.
    Digit0,
    /// Digit one.
    Digit1,
    /// Digit two.
    Digit2,
    /// Digit three.
    Digit3,
    /// Digit four.
    Digit4,
    /// Digit five.
    Digit5,
    /// Digit six.
    Digit6,
    /// Digit seven.
    Digit7,
    /// Digit eight.
    Digit8,
    /// Digit nine.
    Digit9,
    /// Enter key.
    Enter,
    /// Escape key.
    Escape,
    /// Tab key.
    Tab,
    /// Space key.
    Space,
    /// Backspace key.
    Backspace,
    /// Delete key.
    Delete,
    /// Left arrow.
    ArrowLeft,
    /// Right arrow.
    ArrowRight,
    /// Up arrow.
    ArrowUp,
    /// Down arrow.
    ArrowDown,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Function key F1.
    F1,
    /// Function key F2.
    F2,
    /// Function key F3.
    F3,
    /// Function key F4.
    F4,
    /// Function key F5.
    F5,
    /// Function key F6.
    F6,
    /// Function key F7.
    F7,
    /// Function key F8.
    F8,
    /// Function key F9.
    F9,
    /// Function key F10.
    F10,
    /// Function key F11.
    F11,
    /// Function key F12.
    F12,
}

/// One deterministic automation operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationAction {
    /// Invokes one uniquely resolved UI Automation control.
    UiaInvoke {
        /// Exact Automation ID below the approved target window.
        automation_id: String,
    },
    /// Sets the writable Value pattern of one uniquely resolved control.
    UiaSetValue {
        /// Exact Automation ID below the approved target window.
        automation_id: String,
        /// Value passed to the UI Automation Value pattern.
        value: String,
    },
    /// Emits one explicit key chord.
    KeyChord {
        /// Zero to three unique non-Windows modifiers.
        modifiers: Vec<KeyModifier>,
        /// Non-modifier key pressed once.
        key: AutomationKey,
    },
    /// Emits Unicode text using UTF-16 keyboard input records.
    TypeText {
        /// Bounded text emitted exactly as reviewed.
        text: String,
    },
}

impl AutomationAction {
    fn validate(&self) -> Result<(), DomainError> {
        match self {
            Self::UiaInvoke { automation_id } => {
                validate_automation_text("automation id", automation_id, 1, 512)
            }
            Self::UiaSetValue {
                automation_id,
                value,
            } => {
                validate_automation_text("automation id", automation_id, 1, 512)?;
                validate_automation_text("UI Automation value", value, 1, 512)
            }
            Self::KeyChord { modifiers, .. } => {
                if modifiers.len() > 3 {
                    return Err(DomainError::InvalidAutomation(
                        "key chord accepts at most three modifiers".to_owned(),
                    ));
                }
                let mut normalized = modifiers.clone();
                normalized.sort_unstable();
                normalized.dedup();
                if normalized.len() != modifiers.len() {
                    return Err(DomainError::InvalidAutomation(
                        "key chord modifiers must be unique".to_owned(),
                    ));
                }
                Ok(())
            }
            Self::TypeText { text } => validate_automation_text("typed text", text, 1, 512),
        }
    }
}

/// Version-one manual automation plan reviewed and approved as one unit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationPlanV1 {
    /// Exact foreground target.
    pub target: AutomationTarget,
    /// Ordered deterministic actions.
    pub actions: Vec<AutomationAction>,
}

impl AutomationPlanV1 {
    /// Maximum action count accepted by V1.
    pub const MAX_ACTIONS: usize = 10;
    /// Hard execution deadline in seconds.
    pub const EXECUTION_TIMEOUT_SECONDS: u64 = 10;
    /// Minimum pacing between emitted native actions.
    pub const ACTION_PACING_MILLISECONDS: u64 = 100;
    /// One-shot approval lifetime in seconds.
    pub const APPROVAL_TTL_SECONDS: i64 = 60;

    /// Validates the complete plan before hashing, approval, or execution.
    pub fn validate(&self) -> Result<(), DomainError> {
        self.target.validate()?;
        if self.actions.is_empty() || self.actions.len() > Self::MAX_ACTIONS {
            return Err(DomainError::InvalidAutomation(format!(
                "automation plan must contain 1 to {} actions",
                Self::MAX_ACTIONS
            )));
        }
        for action in &self.actions {
            action.validate()?;
        }
        Ok(())
    }

    /// Computes the stable lowercase BLAKE3 digest of the versioned plan encoding.
    pub fn canonical_digest(&self) -> Result<String, DomainError> {
        self.validate()?;
        let encoded = serde_json::to_vec(&("screensearch.automation.v1", self))
            .map_err(|error| DomainError::InvalidAutomation(error.to_string()))?;
        Ok(blake3::hash(&encoded).to_hex().to_string())
    }
}

fn validate_automation_text(
    label: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), DomainError> {
    let length = value.chars().count();
    if length < minimum || length > maximum {
        return Err(DomainError::InvalidAutomation(format!(
            "{label} must contain {minimum} to {maximum} characters"
        )));
    }
    Ok(())
}

/// Durable default-off automation setting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationSettings {
    /// Whether guarded automation may be approved or executed.
    pub enabled: bool,
}

/// Content-free lifecycle of one approval and run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    /// Exact digest approved but not claimed.
    Approved,
    /// Approval was atomically claimed for execution.
    Running,
    /// Every action completed.
    Succeeded,
    /// Execution stopped on a non-abort failure.
    Failed,
    /// Execution stopped because abort was active or recovery found an orphan.
    Aborted,
    /// Approval elapsed before it was claimed.
    Expired,
}

impl AutomationRunStatus {
    /// Stable persistence value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
            Self::Expired => "expired",
        }
    }

    /// Parses a stable persistence value.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "approved" => Ok(Self::Approved),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "aborted" => Ok(Self::Aborted),
            "expired" => Ok(Self::Expired),
            _ => Err(DomainError::InvalidAutomation(format!(
                "unknown automation run status {value}"
            ))),
        }
    }
}

/// Stable content-free automation failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationFailureCode {
    /// Automation is disabled.
    Disabled,
    /// Abort shortcut heartbeat is missing or stale.
    AbortUnavailable,
    /// Emergency abort is latched.
    AbortActive,
    /// Approval does not exist or was already consumed.
    ApprovalMissing,
    /// Approval elapsed before execution.
    ApprovalExpired,
    /// Resubmitted plan digest differs from the approval.
    PlanMismatch,
    /// Foreground HWND, PID, or executable changed.
    TargetChanged,
    /// Interactive session is locked or its state is unknown.
    SessionLocked,
    /// Action pacing or single-flight policy rejected execution.
    RateLimited,
    /// Execution exceeded its deadline.
    Timeout,
    /// Windows rejected or partially injected input.
    InputBlocked,
    /// UI Automation selector found no control.
    ControlMissing,
    /// UI Automation selector found multiple controls.
    ControlAmbiguous,
    /// UI Automation control lacks the requested writable pattern.
    ControlUnsupported,
}

impl AutomationFailureCode {
    /// Stable persistence and IPC value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::AbortUnavailable => "abort_unavailable",
            Self::AbortActive => "abort_active",
            Self::ApprovalMissing => "approval_missing",
            Self::ApprovalExpired => "approval_expired",
            Self::PlanMismatch => "plan_mismatch",
            Self::TargetChanged => "target_changed",
            Self::SessionLocked => "session_locked",
            Self::RateLimited => "rate_limited",
            Self::Timeout => "timeout",
            Self::InputBlocked => "input_blocked",
            Self::ControlMissing => "control_missing",
            Self::ControlAmbiguous => "control_ambiguous",
            Self::ControlUnsupported => "control_unsupported",
        }
    }

    /// Parses a stable persistence or IPC value.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "disabled" => Ok(Self::Disabled),
            "abort_unavailable" => Ok(Self::AbortUnavailable),
            "abort_active" => Ok(Self::AbortActive),
            "approval_missing" => Ok(Self::ApprovalMissing),
            "approval_expired" => Ok(Self::ApprovalExpired),
            "plan_mismatch" => Ok(Self::PlanMismatch),
            "target_changed" => Ok(Self::TargetChanged),
            "session_locked" => Ok(Self::SessionLocked),
            "rate_limited" => Ok(Self::RateLimited),
            "timeout" => Ok(Self::Timeout),
            "input_blocked" => Ok(Self::InputBlocked),
            "control_missing" => Ok(Self::ControlMissing),
            "control_ambiguous" => Ok(Self::ControlAmbiguous),
            "control_unsupported" => Ok(Self::ControlUnsupported),
            _ => Err(DomainError::InvalidAutomation(format!(
                "unknown automation failure code {value}"
            ))),
        }
    }
}

impl std::fmt::Display for AutomationFailureCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Content-free durable approval/run record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationRun {
    /// Approval and run identifier.
    pub id: AutomationRunId,
    /// Canonical plan digest.
    pub plan_digest: String,
    /// Number of ordered actions in the plan.
    pub action_count: u32,
    /// Current lifecycle status.
    pub status: AutomationRunStatus,
    /// Approval creation time.
    pub approved_at: DateTime<Utc>,
    /// Approval expiry.
    pub expires_at: DateTime<Utc>,
    /// Atomic run-claim time.
    pub started_at: Option<DateTime<Utc>>,
    /// Terminal transition time.
    pub finished_at: Option<DateTime<Utc>>,
    /// Stable terminal failure code, when applicable.
    pub failure_code: Option<AutomationFailureCode>,
}

/// An unpersisted screen frame returned by a capture adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct CapturedFrame {
    /// Capture time in UTC.
    pub captured_at: DateTime<Utc>,
    /// Logical monitor identifier supplied by the operating-system adapter.
    pub monitor_id: String,
    /// Foreground executable or application identifier.
    pub application: String,
    /// Foreground window title after privacy filtering.
    pub window_title: String,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
    /// Encoded or raw bytes owned by the capture boundary.
    pub bytes: Vec<u8>,
    /// Media type describing the encoded bytes.
    pub media_type: String,
}

/// Immutable content-addressed asset metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AssetRef {
    /// BLAKE3 content hash.
    pub content_hash: String,
    /// Path relative to the configured asset root.
    pub relative_path: String,
    /// Media type of the stored payload.
    pub media_type: String,
    /// Stored payload size.
    pub byte_length: u64,
}

/// A capture ready for transactional persistence and job enqueueing.
#[derive(Clone, Debug, PartialEq)]
pub struct NewCapture {
    /// Assigned capture identifier.
    pub id: CaptureId,
    /// Capture time in UTC.
    pub captured_at: DateTime<Utc>,
    /// Logical monitor identifier.
    pub monitor_id: String,
    /// Foreground application.
    pub application: String,
    /// Foreground window title.
    pub window_title: String,
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Exact content fingerprint used for idempotency.
    pub fingerprint: String,
    /// Content-addressed asset metadata.
    pub asset: AssetRef,
}

/// Result of a transactional capture insertion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureDisposition {
    /// A new capture and analysis job were committed.
    Enqueued {
        /// Newly persisted capture.
        capture_id: CaptureId,
        /// Durable analysis job created in the same transaction.
        job_id: JobId,
    },
    /// An existing capture had the same fingerprint.
    Duplicate {
        /// Existing capture with the identical fingerprint.
        capture_id: CaptureId,
    },
    /// The frame was intentionally rejected before asset persistence.
    Skipped {
        /// Policy reason that prevented persistence.
        reason: CaptureSkipReason,
    },
}

/// Content-free reason that a capture attempt did not persist pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSkipReason {
    /// Capture was explicitly paused by the user.
    Paused,
    /// The durable analysis queue is above its configured high-water mark.
    Backpressured,
    /// The foreground application matched an exclusion rule.
    ExcludedApplication,
    /// The foreground window title matched an exclusion rule.
    ExcludedTitle,
    /// The frame was below the deterministic perceptual-change threshold.
    NearDuplicate,
}

impl std::fmt::Display for CaptureSkipReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Paused => "paused",
            Self::Backpressured => "backpressured",
            Self::ExcludedApplication => "excluded_application",
            Self::ExcludedTitle => "excluded_title",
            Self::NearDuplicate => "near_duplicate",
        };
        formatter.write_str(value)
    }
}

/// Content-free durable analysis queue measurements.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueueMetrics {
    /// Jobs waiting for a lease.
    pub pending: u64,
    /// Jobs currently holding a lease.
    pub running: u64,
    /// Total failed attempts recorded on live and dead jobs.
    pub retry_count: u64,
    /// Jobs that exhausted their retry budget.
    pub dead_letter_count: u64,
    /// Age of the oldest pending job, rounded down to seconds.
    pub oldest_pending_age_seconds: u64,
}

/// Versioned, user-controlled archive policy stored by the daemon.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArchiveSettings {
    /// Delete eligible captures older than this many days; `None` keeps captures by age.
    pub retention_days: Option<u32>,
    /// Maximum bytes used by immutable capture assets; `None` disables the asset budget.
    pub disk_budget_bytes: Option<u64>,
    /// Case-insensitive application substrings excluded before asset persistence.
    pub excluded_applications: Vec<String>,
    /// Case-insensitive window-title substrings excluded before asset persistence.
    pub excluded_titles: Vec<String>,
}

/// Source used to install or discover a local generation model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSourceKind {
    /// Model was imported from a user-provided local file.
    Local,
    /// Model was downloaded explicitly from Hugging Face.
    HuggingFace,
    /// Model was discovered from packaged application resources.
    Bundled,
}

impl ModelSourceKind {
    /// Stable persistence value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::HuggingFace => "hf",
            Self::Bundled => "bundled",
        }
    }

    /// Parses a persisted source value.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "local" => Ok(Self::Local),
            "hf" => Ok(Self::HuggingFace),
            "bundled" => Ok(Self::Bundled),
            _ => Err(DomainError::InvalidModelCatalog(format!(
                "unknown model source kind {value}"
            ))),
        }
    }
}

/// Runtime metadata for one selectable GGUF generation model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GenerationModel {
    /// Stable model identifier used by settings and diagnostics.
    pub id: String,
    /// Human-readable model name.
    pub display_name: String,
    /// Source used to acquire the model.
    pub source: ModelSourceKind,
    /// Hugging Face repository when `source` is `HuggingFace`.
    pub repository: Option<String>,
    /// File name inside the source repository or import directory.
    pub filename: String,
    /// Path relative to the generation-model root.
    pub relative_path: String,
    /// BLAKE3 hash of the GGUF file when known.
    pub content_hash: Option<String>,
    /// File size in bytes.
    pub byte_length: u64,
    /// Model architecture or family label.
    pub architecture: Option<String>,
    /// Quantization label such as `Q4_K_M`.
    pub quantization: Option<String>,
    /// Context window configured for evaluation.
    pub context_tokens: Option<u32>,
    /// Whether the model has a matching multimodal projector.
    pub supports_vision: bool,
    /// Whether this model is currently active for answer generation.
    pub active: bool,
}

impl GenerationModel {
    /// Validates bounded, content-free model metadata.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_model_text("model id", &self.id, 1, 128)?;
        validate_model_text("model display name", &self.display_name, 1, 160)?;
        validate_model_text("model filename", &self.filename, 1, 260)?;
        validate_model_text("model relative path", &self.relative_path, 1, 512)?;
        if self.relative_path.contains("..") || self.relative_path.starts_with('/') {
            return Err(DomainError::InvalidModelCatalog(
                "model relative path must stay below the model root".to_owned(),
            ));
        }
        if let Some(repository) = &self.repository {
            validate_model_text("model repository", repository, 1, 200)?;
        }
        if let Some(content_hash) = &self.content_hash {
            validate_model_text("model content hash", content_hash, 64, 64)?;
        }
        if self.byte_length == 0 {
            return Err(DomainError::InvalidModelCatalog(
                "model byte length must be non-zero".to_owned(),
            ));
        }
        Ok(())
    }
}

fn validate_model_text(
    label: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), DomainError> {
    let length = value.trim().chars().count();
    if length < minimum || length > maximum {
        return Err(DomainError::InvalidModelCatalog(format!(
            "{label} must contain {minimum} to {maximum} characters"
        )));
    }
    Ok(())
}

impl ArchiveSettings {
    /// Validates bounded settings before they cross persistence or IPC boundaries.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self
            .retention_days
            .is_some_and(|days| days == 0 || days > 3_650)
        {
            return Err(DomainError::InvalidSettings(
                "retention days must be between 1 and 3650".to_owned(),
            ));
        }
        if self
            .disk_budget_bytes
            .is_some_and(|bytes| bytes < 256 * 1024 * 1024)
        {
            return Err(DomainError::InvalidSettings(
                "disk budget must be at least 256 MiB".to_owned(),
            ));
        }
        if self.excluded_applications.len() > 100 || self.excluded_titles.len() > 100 {
            return Err(DomainError::InvalidSettings(
                "at most 100 application and 100 title exclusions are allowed".to_owned(),
            ));
        }
        for pattern in self
            .excluded_applications
            .iter()
            .chain(&self.excluded_titles)
        {
            let length = pattern.trim().chars().count();
            if length == 0 || length > 128 {
                return Err(DomainError::InvalidSettings(
                    "exclusion patterns must contain 1 to 128 characters".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// Content-free archive storage measurements.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StorageMetrics {
    /// Persisted captures.
    pub capture_count: u64,
    /// Immutable assets referenced by at least one capture.
    pub asset_count: u64,
    /// Encoded bytes occupied by referenced capture assets.
    pub asset_bytes: u64,
    /// OCR blocks available for evidence highlighting.
    pub ocr_block_count: u64,
    /// Search chunks indexed lexically and semantically.
    pub search_chunk_count: u64,
}

/// Explicit deletion selector accepted by the archive boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeleteCaptures {
    /// Specific capture identifiers to remove.
    pub capture_ids: Vec<CaptureId>,
    /// Remove captures strictly older than this timestamp.
    pub before: Option<DateTime<Utc>>,
    /// Remove every capture that is not currently leased for analysis.
    pub delete_all: bool,
}

/// Result of retention or explicit deletion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeletionSummary {
    /// Capture rows removed transactionally with their derived data.
    pub captures_deleted: u64,
    /// Newly unreferenced assets placed on the durable cleanup queue.
    pub assets_scheduled: u64,
}

/// One durable, unreferenced asset cleanup task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetCleanupTask {
    /// Content-addressed asset to remove idempotently.
    pub asset: AssetRef,
    /// Number of prior failed cleanup attempts.
    pub attempt: u32,
}

impl QueueMetrics {
    /// Work that is either waiting or currently being processed.
    pub fn depth(self) -> u64 {
        self.pending.saturating_add(self.running)
    }
}

/// A leased background analysis job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisJob {
    /// Job identifier.
    pub id: JobId,
    /// Capture to analyze.
    pub capture_id: CaptureId,
    /// Asset consumed by OCR and vision providers.
    pub asset: AssetRef,
    /// Zero-based retry count.
    pub attempt: u32,
}

/// A normalized rectangle relative to the full capture.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Left coordinate in the range 0 to 1.
    pub x: f32,
    /// Top coordinate in the range 0 to 1.
    pub y: f32,
    /// Width in the range 0 to 1.
    pub width: f32,
    /// Height in the range 0 to 1.
    pub height: f32,
}

impl BoundingBox {
    /// Validates that the rectangle is finite and remains inside the capture.
    pub fn validate(self) -> Result<Self, DomainError> {
        let values = [self.x, self.y, self.width, self.height];
        if values.iter().any(|value| !value.is_finite())
            || self.x < 0.0
            || self.y < 0.0
            || self.width < 0.0
            || self.height < 0.0
            || self.x + self.width > 1.0
            || self.y + self.height > 1.0
        {
            return Err(DomainError::InvalidBoundingBox);
        }
        Ok(self)
    }
}

/// One OCR result in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrBlock {
    /// Zero-based reading order.
    pub reading_order: u32,
    /// Location within the capture.
    pub bounds: BoundingBox,
    /// Recognized text.
    pub text: String,
    /// Recognition confidence from 0 to 1 when exposed by the provider.
    pub confidence: Option<f32>,
    /// BCP-47 language tag when known.
    pub language: Option<String>,
}

/// Indexed text and its vector representation.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexedChunk {
    /// Chunk identifier.
    pub id: ChunkId,
    /// Source capture.
    pub capture_id: CaptureId,
    /// Normalized searchable text.
    pub text: String,
    /// OCR block reading order used to recover positioned evidence.
    pub source_reading_order: u32,
    /// Embedding model revision.
    pub model_id: String,
    /// Fixed-dimension vector.
    pub embedding: Vec<f32>,
}

/// Retrieval paths contributing to a fused search hit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchMatchKind {
    /// Full-text retrieval only.
    Lexical,
    /// Vector retrieval only.
    Semantic,
    /// Both full-text and vector retrieval.
    Hybrid,
}

/// Atomic result written when an analysis job succeeds.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisResult {
    /// Completed job.
    pub job_id: JobId,
    /// Source capture.
    pub capture_id: CaptureId,
    /// OCR blocks in reading order.
    pub blocks: Vec<OcrBlock>,
    /// Search chunks with embeddings.
    pub chunks: Vec<IndexedChunk>,
    /// OCR model revision.
    pub ocr_model_id: String,
}

/// One ranked hybrid retrieval result.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    /// Source chunk.
    pub chunk_id: ChunkId,
    /// Source capture.
    pub capture_id: CaptureId,
    /// Text passed to context assembly.
    pub text: String,
    /// Reciprocal-rank-fusion score.
    pub score: f64,
    /// Capture time in UTC.
    pub captured_at: DateTime<Utc>,
    /// Foreground application recorded with the capture.
    pub application: String,
    /// Privacy-filtered foreground window title.
    pub window_title: String,
    /// Capture width in pixels.
    pub width: u32,
    /// Capture height in pixels.
    pub height: u32,
    /// Immutable screenshot asset.
    pub asset: AssetRef,
    /// Positioned OCR regions supporting this hit.
    pub bounds: Vec<BoundingBox>,
    /// Retrieval paths contributing to the rank.
    pub match_kind: SearchMatchKind,
    /// OCR provider revision used for the evidence.
    pub ocr_model_id: String,
    /// Embedding provider revision used for semantic ranking.
    pub embedding_model_id: String,
}

/// Event emitted by citation-aware answer generation.
#[derive(Clone, Debug, PartialEq)]
pub enum SearchEvent {
    /// Retrieval evidence, emitted before answer tokens.
    Citation(Box<SearchHit>),
    /// One incremental text token or token group.
    Token(String),
    /// Terminal event containing the number of citations.
    Completed {
        /// Number of retrieval citations emitted for this answer.
        citation_count: usize,
        /// Content-free answer-generation terminal status.
        answer_status: String,
        /// Optional content-free explanation for the answer status.
        answer_message: Option<String>,
    },
}

/// Domain validation failures.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DomainError {
    /// A bounding box was non-finite or outside normalized coordinates.
    #[error("bounding box must be finite and within normalized capture coordinates")]
    InvalidBoundingBox,
    /// A request that requires text received only whitespace.
    #[error("text must not be empty")]
    EmptyText,
    /// Archive settings exceeded a documented bound.
    #[error("invalid archive settings: {0}")]
    InvalidSettings(String),
    /// Model catalog metadata exceeded a documented bound.
    #[error("invalid model catalog: {0}")]
    InvalidModelCatalog(String),
    /// Automation input or persisted automation metadata violated V1 bounds.
    #[error("invalid automation: {0}")]
    InvalidAutomation(String),
}

#[cfg(test)]
mod tests {
    use super::{
        ArchiveSettings, AutomationAction, AutomationKey, AutomationPlanV1, AutomationTarget,
        BoundingBox, DomainError, GenerationModel, KeyModifier, ModelSourceKind,
    };

    fn valid_generation_model() -> GenerationModel {
        GenerationModel {
            id: "qwen2-5-1-5b-instruct".to_owned(),
            display_name: "Qwen2.5 1.5B Instruct".to_owned(),
            source: ModelSourceKind::Local,
            repository: None,
            filename: "qwen2.5-1.5b-instruct-q4_k_m.gguf".to_owned(),
            relative_path: "qwen2-5-1-5b-instruct/qwen2.5-1.5b-instruct-q4_k_m.gguf".to_owned(),
            content_hash: Some("a".repeat(64)),
            byte_length: 1_024,
            architecture: Some("Qwen".to_owned()),
            quantization: Some("Q4_K_M".to_owned()),
            context_tokens: Some(2_048),
            supports_vision: false,
            active: false,
        }
    }

    #[test]
    fn generation_model_accepts_bounded_metadata() {
        assert!(valid_generation_model().validate().is_ok());
    }

    #[test]
    fn generation_model_rejects_relative_path_escapes() {
        let mut traversal = valid_generation_model();
        traversal.relative_path = "../escape/model.gguf".to_owned();
        assert!(matches!(
            traversal.validate(),
            Err(DomainError::InvalidModelCatalog(_))
        ));

        let mut absolute = valid_generation_model();
        absolute.relative_path = "/etc/model.gguf".to_owned();
        assert!(matches!(
            absolute.validate(),
            Err(DomainError::InvalidModelCatalog(_))
        ));
    }

    #[test]
    fn generation_model_rejects_out_of_range_fields() {
        let mut empty_id = valid_generation_model();
        empty_id.id = String::new();
        assert!(empty_id.validate().is_err());

        let mut short_hash = valid_generation_model();
        short_hash.content_hash = Some("abc".to_owned());
        assert!(short_hash.validate().is_err());

        let mut zero_bytes = valid_generation_model();
        zero_bytes.byte_length = 0;
        assert!(zero_bytes.validate().is_err());
    }

    #[test]
    fn generation_model_accepts_a_full_length_content_hash() {
        let mut model = valid_generation_model();
        model.content_hash = Some("f".repeat(64));
        assert!(model.validate().is_ok());
    }

    #[test]
    fn model_source_kind_round_trips_through_its_persistence_value() {
        for kind in [
            ModelSourceKind::Local,
            ModelSourceKind::HuggingFace,
            ModelSourceKind::Bundled,
        ] {
            assert_eq!(ModelSourceKind::parse(kind.as_str()), Ok(kind));
        }
        assert!(ModelSourceKind::parse("totally-unknown").is_err());
    }

    #[test]
    fn bounding_box_rejects_coordinates_outside_capture() {
        let bounds = BoundingBox {
            x: 0.8,
            y: 0.0,
            width: 0.3,
            height: 1.0,
        };

        assert!(bounds.validate().is_err());
    }

    #[test]
    fn archive_settings_reject_unbounded_or_empty_values() {
        assert!(
            ArchiveSettings {
                retention_days: Some(0),
                ..ArchiveSettings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ArchiveSettings {
                excluded_titles: vec![String::new()],
                ..ArchiveSettings::default()
            }
            .validate()
            .is_err()
        );
    }

    fn automation_target() -> AutomationTarget {
        AutomationTarget {
            process_id: 42,
            window_handle: 9001,
            executable_name: "notepad.exe".to_owned(),
            display_title: "Notes".to_owned(),
        }
    }

    fn automation_plan(actions: Vec<AutomationAction>) -> AutomationPlanV1 {
        AutomationPlanV1 {
            target: automation_target(),
            actions,
        }
    }

    #[test]
    fn automation_plan_accepts_the_bounded_typed_action_set() {
        let plan = automation_plan(vec![
            AutomationAction::UiaInvoke {
                automation_id: "save".to_owned(),
            },
            AutomationAction::UiaSetValue {
                automation_id: "name".to_owned(),
                value: "Quarterly notes".to_owned(),
            },
            AutomationAction::KeyChord {
                modifiers: vec![KeyModifier::Control, KeyModifier::Shift],
                key: AutomationKey::S,
            },
            AutomationAction::TypeText {
                text: "Ready".to_owned(),
            },
        ]);

        assert!(plan.validate().is_ok());
        assert_eq!(plan.canonical_digest().unwrap().len(), 64);
        assert_eq!(
            plan.canonical_digest().unwrap(),
            plan.clone().canonical_digest().unwrap()
        );
    }

    #[test]
    fn automation_plan_rejects_empty_and_oversized_action_lists() {
        assert!(automation_plan(Vec::new()).validate().is_err());
        assert!(
            automation_plan(
                (0..11)
                    .map(|_| AutomationAction::TypeText {
                        text: "x".to_owned()
                    })
                    .collect()
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn automation_plan_rejects_oversized_content_and_duplicate_modifiers() {
        assert!(
            automation_plan(vec![AutomationAction::TypeText {
                text: "x".repeat(513)
            }])
            .validate()
            .is_err()
        );
        assert!(
            automation_plan(vec![AutomationAction::KeyChord {
                modifiers: vec![KeyModifier::Control, KeyModifier::Control],
                key: AutomationKey::Enter,
            }])
            .validate()
            .is_err()
        );
    }

    #[test]
    fn automation_digest_is_order_sensitive() {
        let first = automation_plan(vec![
            AutomationAction::TypeText {
                text: "one".to_owned(),
            },
            AutomationAction::TypeText {
                text: "two".to_owned(),
            },
        ]);
        let second = automation_plan(vec![
            AutomationAction::TypeText {
                text: "two".to_owned(),
            },
            AutomationAction::TypeText {
                text: "one".to_owned(),
            },
        ]);

        assert_ne!(
            first.canonical_digest().unwrap(),
            second.canonical_digest().unwrap()
        );
    }
}
