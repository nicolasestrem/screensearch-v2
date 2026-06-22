CREATE TABLE automation_settings_v1 (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0, 1)),
    updated_at TEXT NOT NULL
);

INSERT INTO automation_settings_v1(singleton, enabled, updated_at)
VALUES (1, 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

CREATE TABLE automation_run_v2 (
    id TEXT PRIMARY KEY,
    plan_digest TEXT NOT NULL CHECK(length(plan_digest) = 64),
    action_count INTEGER NOT NULL CHECK(action_count BETWEEN 1 AND 10),
    status TEXT NOT NULL CHECK(
        status IN ('approved', 'running', 'succeeded', 'failed', 'aborted', 'expired')
    ),
    approved_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    failure_code TEXT CHECK(
        failure_code IS NULL OR failure_code IN (
            'disabled',
            'abort_unavailable',
            'abort_active',
            'approval_missing',
            'approval_expired',
            'plan_mismatch',
            'target_changed',
            'session_locked',
            'rate_limited',
            'timeout',
            'input_blocked',
            'control_missing',
            'control_ambiguous',
            'control_unsupported'
        )
    )
);

CREATE INDEX automation_run_v2_status_idx
ON automation_run_v2(status, expires_at);

INSERT INTO schema_migration(version, applied_at)
VALUES (7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
