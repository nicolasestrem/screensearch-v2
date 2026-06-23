//! Guarded automation service safety and lifecycle contract tests.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration as StdDuration,
};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use futures::lock::Mutex;
use screensearch_application::{AutomationService, AutomationServiceConfig};
use screensearch_domain::{
    AutomationAction, AutomationFailureCode, AutomationKey, AutomationPlanV1, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationSettings, AutomationTarget,
};
use screensearch_ports::{
    AutomationAbortSignal, AutomationClaimOutcome, AutomationPlatform, AutomationRepository,
    PortError,
};

#[derive(Default)]
struct MemoryAutomationRepository {
    settings: Mutex<AutomationSettings>,
    runs: Mutex<HashMap<AutomationRunId, AutomationRun>>,
}

#[async_trait]
impl AutomationRepository for MemoryAutomationRepository {
    async fn automation_settings(&self) -> Result<AutomationSettings, PortError> {
        Ok(*self.settings.lock().await)
    }

    async fn update_automation_settings(
        &self,
        settings: AutomationSettings,
    ) -> Result<(), PortError> {
        *self.settings.lock().await = settings;
        Ok(())
    }

    async fn create_automation_approval(&self, run: AutomationRun) -> Result<(), PortError> {
        self.runs.lock().await.insert(run.id, run);
        Ok(())
    }

    async fn claim_automation_run(
        &self,
        id: AutomationRunId,
        plan_digest: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<AutomationClaimOutcome, PortError> {
        let mut runs = self.runs.lock().await;
        let Some(run) = runs.get_mut(&id) else {
            return Ok(AutomationClaimOutcome::Missing);
        };
        if run.status != AutomationRunStatus::Approved {
            return Ok(AutomationClaimOutcome::Missing);
        }
        if run.plan_digest != plan_digest {
            return Ok(AutomationClaimOutcome::PlanMismatch);
        }
        if run.expires_at <= now {
            run.status = AutomationRunStatus::Expired;
            return Ok(AutomationClaimOutcome::Expired);
        }
        run.status = AutomationRunStatus::Running;
        run.started_at = Some(now);
        Ok(AutomationClaimOutcome::Claimed(run.clone()))
    }

    async fn finish_automation_run(
        &self,
        id: AutomationRunId,
        status: AutomationRunStatus,
        failure_code: Option<AutomationFailureCode>,
        finished_at: chrono::DateTime<Utc>,
    ) -> Result<(), PortError> {
        let mut runs = self.runs.lock().await;
        let run = runs.get_mut(&id).unwrap();
        run.status = status;
        run.failure_code = failure_code;
        run.finished_at = Some(finished_at);
        Ok(())
    }

    async fn automation_run(
        &self,
        id: AutomationRunId,
    ) -> Result<Option<AutomationRun>, PortError> {
        Ok(self.runs.lock().await.get(&id).cloned())
    }

    async fn recover_automation_runs(
        &self,
        recovered_at: chrono::DateTime<Utc>,
    ) -> Result<u64, PortError> {
        let mut recovered = 0;
        for run in self.runs.lock().await.values_mut() {
            if run.status == AutomationRunStatus::Running {
                run.status = AutomationRunStatus::Aborted;
                run.finished_at = Some(recovered_at);
                recovered += 1;
            }
        }
        Ok(recovered)
    }
}

struct FakePlatform {
    target: Mutex<AutomationTarget>,
    unlocked: Mutex<bool>,
    action_delay: StdDuration,
    emitted: AtomicUsize,
}

impl FakePlatform {
    fn new(target: AutomationTarget) -> Self {
        Self {
            target: Mutex::new(target),
            unlocked: Mutex::new(true),
            action_delay: StdDuration::ZERO,
            emitted: AtomicUsize::new(0),
        }
    }

    fn delayed(target: AutomationTarget, action_delay: StdDuration) -> Self {
        Self {
            action_delay,
            ..Self::new(target)
        }
    }
}

struct BlockingPlatform {
    target: AutomationTarget,
    action_delay: StdDuration,
    emitted: Arc<AtomicUsize>,
}

impl BlockingPlatform {
    fn new(target: AutomationTarget, action_delay: StdDuration) -> Self {
        Self {
            target,
            action_delay,
            emitted: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl AutomationPlatform for BlockingPlatform {
    async fn foreground_target(&self) -> Result<AutomationTarget, PortError> {
        Ok(self.target.clone())
    }

    async fn session_is_unlocked(&self) -> Result<bool, PortError> {
        Ok(true)
    }

    async fn execute_action(
        &self,
        _target: &AutomationTarget,
        _action: &AutomationAction,
        abort_signal: AutomationAbortSignal,
    ) -> Result<(), PortError> {
        let emitted = Arc::clone(&self.emitted);
        let delay = self.action_delay;
        tokio::task::spawn_blocking(move || {
            std::thread::sleep(delay);
            if abort_signal.is_cancelled() {
                return Err(PortError::Automation(AutomationFailureCode::AbortActive));
            }
            emitted.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
        .map_err(|error| PortError::Internal(format!("blocking fake failed: {error}")))?
    }
}

#[async_trait]
impl AutomationPlatform for FakePlatform {
    async fn foreground_target(&self) -> Result<AutomationTarget, PortError> {
        Ok(self.target.lock().await.clone())
    }

    async fn session_is_unlocked(&self) -> Result<bool, PortError> {
        Ok(*self.unlocked.lock().await)
    }

    async fn execute_action(
        &self,
        _target: &AutomationTarget,
        _action: &AutomationAction,
        abort_signal: AutomationAbortSignal,
    ) -> Result<(), PortError> {
        tokio::time::sleep(self.action_delay).await;
        if abort_signal.is_cancelled() {
            return Err(PortError::Automation(AutomationFailureCode::AbortActive));
        }
        self.emitted.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn target() -> AutomationTarget {
    AutomationTarget {
        process_id: 42,
        window_handle: 9001,
        executable_name: "fixture.exe".to_owned(),
        display_title: "Automation fixture".to_owned(),
    }
}

fn plan() -> AutomationPlanV1 {
    AutomationPlanV1 {
        target: target(),
        actions: vec![AutomationAction::KeyChord {
            modifiers: Vec::new(),
            key: AutomationKey::Enter,
        }],
    }
}

fn fast_config() -> AutomationServiceConfig {
    AutomationServiceConfig {
        heartbeat_stale_after: StdDuration::from_millis(50),
        execution_timeout: StdDuration::from_millis(50),
        action_pacing: StdDuration::from_millis(1),
        approval_ttl: Duration::seconds(60),
    }
}

async fn ready_service(
    platform: Arc<FakePlatform>,
) -> (
    Arc<MemoryAutomationRepository>,
    AutomationService,
    chrono::DateTime<Utc>,
) {
    let repository = Arc::new(MemoryAutomationRepository::default());
    let service = AutomationService::with_config(repository.clone(), platform, fast_config());
    let now = Utc::now();
    service.safety_heartbeat(true, now).await;
    service.set_enabled(true, now).await.unwrap();
    (repository, service, now)
}

#[tokio::test]
async fn automation_is_default_off_and_requires_a_live_abort_heartbeat() {
    let repository = Arc::new(MemoryAutomationRepository::default());
    let platform = Arc::new(FakePlatform::new(target()));
    let service = AutomationService::with_config(repository, platform, fast_config());
    let now = Utc::now();

    assert_eq!(
        service.approve(plan(), now).await,
        Err(PortError::Automation(AutomationFailureCode::Disabled))
    );
    assert_eq!(
        service.set_enabled(true, now).await,
        Err(PortError::Automation(
            AutomationFailureCode::AbortUnavailable
        ))
    );
}

#[tokio::test]
async fn approval_rejects_locked_sessions_and_focus_drift() {
    let platform = Arc::new(FakePlatform::new(target()));
    let (_, service, now) = ready_service(platform.clone()).await;
    *platform.unlocked.lock().await = false;
    assert_eq!(
        service.approve(plan(), now).await,
        Err(PortError::Automation(AutomationFailureCode::SessionLocked))
    );

    *platform.unlocked.lock().await = true;
    platform.target.lock().await.window_handle = 7;
    assert_eq!(
        service.approve(plan(), now).await,
        Err(PortError::Automation(AutomationFailureCode::TargetChanged))
    );
}

#[tokio::test]
async fn execution_is_one_shot_and_records_success() {
    let platform = Arc::new(FakePlatform::new(target()));
    let (repository, service, now) = ready_service(platform.clone()).await;
    let approval = service.approve(plan(), now).await.unwrap();

    service.execute(approval.id, plan()).await.unwrap();
    assert_eq!(platform.emitted.load(Ordering::SeqCst), 1);
    assert_eq!(
        repository
            .automation_run(approval.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationRunStatus::Succeeded
    );
    assert_eq!(
        service.execute(approval.id, plan()).await,
        Err(PortError::Automation(
            AutomationFailureCode::ApprovalMissing
        ))
    );
}

#[tokio::test]
async fn abort_latches_and_blocks_execution_until_explicit_reset() {
    let platform = Arc::new(FakePlatform::new(target()));
    let (_, service, now) = ready_service(platform).await;
    let approval = service.approve(plan(), now).await.unwrap();
    service.abort();

    assert_eq!(
        service.execute(approval.id, plan()).await,
        Err(PortError::Automation(AutomationFailureCode::AbortActive))
    );
    service.reset_abort();
    assert!(!service.status(Utc::now()).await.unwrap().abort_active);
}

#[tokio::test]
async fn concurrent_and_timed_out_execution_fail_closed() {
    let platform = Arc::new(FakePlatform::delayed(
        target(),
        StdDuration::from_millis(100),
    ));
    let (_, service, now) = ready_service(platform).await;
    let first = service.approve(plan(), now).await.unwrap();
    let second = service
        .approve(plan(), now + Duration::milliseconds(1))
        .await
        .unwrap();
    let first_execution = service.execute(first.id, plan());
    let second_execution = service.execute(second.id, plan());
    let (first_result, second_result) = tokio::join!(first_execution, second_execution);

    assert_eq!(
        first_result,
        Err(PortError::Automation(AutomationFailureCode::Timeout))
    );
    assert_eq!(
        second_result,
        Err(PortError::Automation(AutomationFailureCode::RateLimited))
    );
}

#[tokio::test]
async fn abort_cancels_in_flight_platform_action() {
    let platform = Arc::new(FakePlatform::delayed(
        target(),
        StdDuration::from_millis(100),
    ));
    let repository = Arc::new(MemoryAutomationRepository::default());
    let service = AutomationService::with_config(
        repository,
        platform.clone(),
        AutomationServiceConfig {
            execution_timeout: StdDuration::from_secs(1),
            ..fast_config()
        },
    );
    let now = Utc::now();
    service.safety_heartbeat(true, now).await;
    service.set_enabled(true, now).await.unwrap();
    let approval = service.approve(plan(), now).await.unwrap();

    let execution = service.execute(approval.id, plan());
    tokio::time::sleep(StdDuration::from_millis(10)).await;
    service.abort();

    assert_eq!(
        execution.await,
        Err(PortError::Automation(AutomationFailureCode::AbortActive))
    );
    assert_eq!(platform.emitted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn timeout_cancels_late_blocking_platform_emission() {
    let platform = Arc::new(BlockingPlatform::new(
        target(),
        StdDuration::from_millis(100),
    ));
    let emitted = Arc::clone(&platform.emitted);
    let repository = Arc::new(MemoryAutomationRepository::default());
    let service = AutomationService::with_config(
        repository,
        platform,
        AutomationServiceConfig {
            execution_timeout: StdDuration::from_millis(10),
            ..fast_config()
        },
    );
    let now = Utc::now();
    service.safety_heartbeat(true, now).await;
    service.set_enabled(true, now).await.unwrap();
    let approval = service.approve(plan(), now).await.unwrap();

    assert_eq!(
        service.execute(approval.id, plan()).await,
        Err(PortError::Automation(AutomationFailureCode::Timeout))
    );
    tokio::time::sleep(StdDuration::from_millis(150)).await;
    assert_eq!(emitted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn approval_requires_the_target_to_be_foreground_not_the_shell() {
    // The daemon's approval-time foreground check compares the captured target against whatever is
    // foreground *now*. If the desktop shell were foreground (a different window) at approve time,
    // approval must fail closed — which is why the shell hides itself before approving, exactly as
    // it does before capture and execute.
    let platform = Arc::new(FakePlatform::new(target()));
    let (_, service, now) = ready_service(platform.clone()).await;
    *platform.target.lock().await = AutomationTarget {
        process_id: 4321,
        window_handle: 5555,
        executable_name: "screensearch.exe".to_owned(),
        display_title: "ScreenSearch".to_owned(),
    };
    assert_eq!(
        service.approve(plan(), now).await,
        Err(PortError::Automation(AutomationFailureCode::TargetChanged))
    );
}

#[tokio::test]
async fn execution_accepts_title_only_changes() {
    // A window may legitimately retitle itself between approve and execute. Because the canonical
    // digest excludes the volatile display title, a title-only difference must not be rejected as a
    // plan mismatch.
    let platform = Arc::new(FakePlatform::new(target()));
    let (repository, service, now) = ready_service(platform).await;
    let approval = service.approve(plan(), now).await.unwrap();

    let mut retitled = plan();
    retitled.target.display_title = "A new window title".to_owned();
    service.execute(approval.id, retitled).await.unwrap();

    assert_eq!(
        repository
            .automation_run(approval.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationRunStatus::Succeeded
    );
}

#[tokio::test]
async fn status_distinguishes_unregistered_from_stale_heartbeat() {
    let repository = Arc::new(MemoryAutomationRepository::default());
    let platform = Arc::new(FakePlatform::new(target()));
    let service = AutomationService::with_config(repository, platform, fast_config());
    let now = Utc::now();

    // No heartbeat yet: nothing is fresh.
    let status = service.status(now).await.unwrap();
    assert!(!status.heartbeat_fresh && !status.abort_registered && !status.abort_available);

    // Fresh heartbeat that reports the shortcut unregistered: fresh, but not available.
    service.safety_heartbeat(false, now).await;
    let status = service.status(now).await.unwrap();
    assert!(status.heartbeat_fresh && !status.abort_registered && !status.abort_available);

    // Fresh and registered: available.
    service.safety_heartbeat(true, now).await;
    let status = service.status(now).await.unwrap();
    assert!(status.heartbeat_fresh && status.abort_registered && status.abort_available);

    // A heartbeat older than the staleness window is no longer fresh, so abort is unavailable.
    let stale = now + Duration::milliseconds(200);
    let status = service.status(stale).await.unwrap();
    assert!(!status.heartbeat_fresh && !status.abort_available);
}
