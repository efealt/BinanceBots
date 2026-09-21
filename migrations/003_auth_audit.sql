CREATE TABLE auth_audit_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL CHECK (event_type IN ('login_success', 'login_failed', 'logout')),
    occurred_at_ms INTEGER NOT NULL,
    source_ip TEXT,
    user_agent TEXT
);

CREATE INDEX idx_auth_audit_events_occurred
ON auth_audit_events (occurred_at_ms DESC, event_id DESC);
