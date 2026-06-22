use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration as StdDuration,
};

use chrono::{DateTime, Duration, Utc};
use futures::lock::Mutex;
use screensearch_domain::{
    AutomationFailureCode, AutomationPlanV1, AutomationRun, AutomationRunId, AutomationRunStatus,
    AutomationSettings, AutomationTarget,
};
use screensearch_ports::{
    AutomationClaimOutcome, AutomationPlatform, AutomationRepository, PortError,
};

/// Guarded automation timing policy.
#[derive(Clone, Copy, Debug)]
pub struct AutomationServiceConfig {
    /// Maximum age of the desktop shell abort-registration heartbeat.
    pub heartbeat_stale_after: StdDuration,
    /// Hard deadline for one claimed plan.
    pub execution_timeout: StdDuration,
    /// Minimum delay between two ordered native actions.
    pub action_pacing: StdDuration,
    /// Lifetime of one exact-plan approval.
    pub approval_ttl: Duration,
}

impl Default for AutomationServiceConfig {
    fn default() -> Self {
        Self {
            heartbeat_stale_after: StdDuration::from_secs(10),
            execution_timeout: StdDuration::from_secs(AutomationPlanV1::EXECUTION_TIMEOUT_SECONDS),
            action_pacing: StdDuration::from_millis(AutomationPlanV1::ACTION_PACING_MILLISECONDS),
            approval_ttl: Duration::seconds(AutomationPlanV1::APPROVAL_TTL_SECONDS),
        }
    }
}

/// Content-free guarded automation state exposed to IPC and UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct AutomationServiceStatus {
    /// Durable daemon-owned enablement.
    pub enabled: bool,
    /// Whether the shell abort registration has a fresh heartbeat.
    pub abort_available: bool,
    /// Whether emergency abort is latched.
    pub abort_active: bool,
    /// Whether one plan currently owns the single-flight gate.
    pub running: bool,
}

/// Daemon-owned approval, safety, and execution orchestration.
pub struct AutomationService {
    repository: Arc<dyn AutomationRepository>,
    platform: Arc<dyn AutomationPlatform>,
    config: AutomationServiceConfig,
    heartbeat: Mutex<Option<DateTime<Utc>>>,
    abort_latched: AtomicBool,
    executing: AtomicBool,
}

impl AutomationService {
    /// Creates the production service with the fixed V1 timing policy.
    pub fn new(
        repository: Arc<dyn AutomationRepository>,
        platform: Arc<dyn AutomationPlatform>,
    ) -> Self {
        Self::with_config(repository, platform, AutomationServiceConfig::default())
    }

    /// Creates a service with explicit timing values for deterministic contract tests.
    pub fn with_config(
        repository: Arc<dyn AutomationRepository>,
        platform: Arc<dyn AutomationPlatform>,
        config: AutomationServiceConfig,
    ) -> Self {
        Self {
            repository,
            platform,
            config,
            heartbeat: Mutex::new(None),
            abort_latched: AtomicBool::new(false),
            executing: AtomicBool::new(false),
        }
    }

    /// Recovers orphaned running rows after daemon startup.
    pub async fn recover_startup(&self, now: DateTime<Utc>) -> Result<u64, PortError> {
        self.repository.recover_automation_runs(now).await
    }

    /// Records whether the desktop shell currently owns the fixed abort shortcut.
    pub async fn safety_heartbeat(&self, abort_registered: bool, at: DateTime<Utc>) {
        *self.heartbeat.lock().await = abort_registered.then_some(at);
    }

    /// Enables or disables automation; enabling requires a live abort registration.
    pub async fn set_enabled(&self, enabled: bool, now: DateTime<Utc>) -> Result<(), PortError> {
        if enabled {
            self.ensure_abort_ready(now).await?;
            if self.abort_latched.load(Ordering::SeqCst) {
                return Err(automation_failure(AutomationFailureCode::AbortActive));
            }
        }
        self.repository
            .update_automation_settings(AutomationSettings { enabled })
            .await
    }

    /// Returns content-free live guarded automation state.
    pub async fn status(&self, now: DateTime<Utc>) -> Result<AutomationServiceStatus, PortError> {
        Ok(AutomationServiceStatus {
            enabled: self.repository.automation_settings().await?.enabled,
            abort_available: self.abort_available(now).await,
            abort_active: self.abort_latched.load(Ordering::SeqCst),
            running: self.executing.load(Ordering::SeqCst),
        })
    }

    /// Captures the current foreground target after enablement and safety checks.
    pub async fn foreground_target(
        &self,
        now: DateTime<Utc>,
    ) -> Result<AutomationTarget, PortError> {
        self.ensure_ready(now).await?;
        if !self.platform.session_is_unlocked().await.unwrap_or(false) {
            return Err(automation_failure(AutomationFailureCode::SessionLocked));
        }
        self.platform
            .foreground_target()
            .await
            .map_err(|_| automation_failure(AutomationFailureCode::TargetChanged))
    }

    /// Persists a one-shot approval for the exact canonical plan digest.
    pub async fn approve(
        &self,
        plan: AutomationPlanV1,
        now: DateTime<Utc>,
    ) -> Result<AutomationRun, PortError> {
        plan.validate()
            .map_err(|error| PortError::InvalidData(error.to_string()))?;
        self.check_target(now, &plan.target).await?;
        let run = AutomationRun {
            id: AutomationRunId::new(),
            plan_digest: plan
                .canonical_digest()
                .map_err(|error| PortError::InvalidData(error.to_string()))?,
            action_count: u32::try_from(plan.actions.len())
                .map_err(|_| PortError::InvalidData("too many automation actions".to_owned()))?,
            status: AutomationRunStatus::Approved,
            approved_at: now,
            expires_at: now + self.config.approval_ttl,
            started_at: None,
            finished_at: None,
            failure_code: None,
        };
        self.repository
            .create_automation_approval(run.clone())
            .await?;
        Ok(run)
    }

    /// Claims and executes one exact, approved plan.
    pub async fn execute(
        &self,
        approval_id: AutomationRunId,
        plan: AutomationPlanV1,
    ) -> Result<(), PortError> {
        let _execution = ExecutionGuard::acquire(&self.executing)?;
        let digest = plan
            .canonical_digest()
            .map_err(|error| PortError::InvalidData(error.to_string()))?;
        let now = Utc::now();
        self.check_target(now, &plan.target).await?;
        match self
            .repository
            .claim_automation_run(approval_id, &digest, now)
            .await?
        {
            AutomationClaimOutcome::Claimed(_) => {}
            AutomationClaimOutcome::Missing => {
                return Err(automation_failure(AutomationFailureCode::ApprovalMissing));
            }
            AutomationClaimOutcome::Expired => {
                return Err(automation_failure(AutomationFailureCode::ApprovalExpired));
            }
            AutomationClaimOutcome::PlanMismatch => {
                return Err(automation_failure(AutomationFailureCode::PlanMismatch));
            }
        }

        let result =
            match tokio::time::timeout(self.config.execution_timeout, self.execute_actions(&plan))
                .await
            {
                Ok(result) => result,
                Err(_) => Err(automation_failure(AutomationFailureCode::Timeout)),
            };
        let (status, failure_code) = match &result {
            Ok(()) => (AutomationRunStatus::Succeeded, None),
            Err(PortError::Automation(AutomationFailureCode::AbortActive)) => (
                AutomationRunStatus::Aborted,
                Some(AutomationFailureCode::AbortActive),
            ),
            Err(PortError::Automation(code)) => (AutomationRunStatus::Failed, Some(*code)),
            Err(_) => (
                AutomationRunStatus::Failed,
                Some(AutomationFailureCode::InputBlocked),
            ),
        };
        self.repository
            .finish_automation_run(approval_id, status, failure_code, Utc::now())
            .await?;
        result
    }

    /// Latches emergency abort. The latch remains active until explicit reset.
    pub fn abort(&self) {
        self.abort_latched.store(true, Ordering::SeqCst);
    }

    /// Explicitly clears the emergency-abort latch.
    pub fn reset_abort(&self) {
        self.abort_latched.store(false, Ordering::SeqCst);
    }

    async fn execute_actions(&self, plan: &AutomationPlanV1) -> Result<(), PortError> {
        for (index, action) in plan.actions.iter().enumerate() {
            if index > 0 {
                tokio::time::sleep(self.config.action_pacing).await;
            }
            self.check_target(Utc::now(), &plan.target).await?;
            self.platform
                .execute_action(&plan.target, action)
                .await
                .map_err(|error| match error {
                    PortError::Automation(_) => error,
                    _ => automation_failure(AutomationFailureCode::InputBlocked),
                })?;
        }
        Ok(())
    }

    async fn check_target(
        &self,
        now: DateTime<Utc>,
        expected: &AutomationTarget,
    ) -> Result<(), PortError> {
        self.ensure_ready(now).await?;
        if !self.platform.session_is_unlocked().await.unwrap_or(false) {
            return Err(automation_failure(AutomationFailureCode::SessionLocked));
        }
        let foreground = self
            .platform
            .foreground_target()
            .await
            .map_err(|_| automation_failure(AutomationFailureCode::TargetChanged))?;
        if !same_target_identity(&foreground, expected) {
            return Err(automation_failure(AutomationFailureCode::TargetChanged));
        }
        Ok(())
    }

    async fn ensure_ready(&self, now: DateTime<Utc>) -> Result<(), PortError> {
        if !self.repository.automation_settings().await?.enabled {
            return Err(automation_failure(AutomationFailureCode::Disabled));
        }
        self.ensure_abort_ready(now).await?;
        if self.abort_latched.load(Ordering::SeqCst) {
            return Err(automation_failure(AutomationFailureCode::AbortActive));
        }
        Ok(())
    }

    async fn ensure_abort_ready(&self, now: DateTime<Utc>) -> Result<(), PortError> {
        if !self.abort_available(now).await {
            return Err(automation_failure(AutomationFailureCode::AbortUnavailable));
        }
        Ok(())
    }

    async fn abort_available(&self, now: DateTime<Utc>) -> bool {
        let Some(heartbeat) = *self.heartbeat.lock().await else {
            return false;
        };
        now.signed_duration_since(heartbeat)
            .to_std()
            .is_ok_and(|age| age <= self.config.heartbeat_stale_after)
    }
}

fn automation_failure(code: AutomationFailureCode) -> PortError {
    PortError::Automation(code)
}

fn same_target_identity(left: &AutomationTarget, right: &AutomationTarget) -> bool {
    left.process_id == right.process_id
        && left.window_handle == right.window_handle
        && left
            .executable_name
            .eq_ignore_ascii_case(&right.executable_name)
}

struct ExecutionGuard<'a> {
    executing: &'a AtomicBool,
}

impl<'a> ExecutionGuard<'a> {
    fn acquire(executing: &'a AtomicBool) -> Result<Self, PortError> {
        executing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| automation_failure(AutomationFailureCode::RateLimited))?;
        Ok(Self { executing })
    }
}

impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        self.executing.store(false, Ordering::SeqCst);
    }
}
