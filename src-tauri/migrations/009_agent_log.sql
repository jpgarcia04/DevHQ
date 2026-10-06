-- What agents changed through the agent gate in what carries no source of its own: applications,
-- resources, components and HTTP checks, which are configuration, not evidence. Observations,
-- incidents and events say who reported them in their own `source`, so they are not repeated
-- here. Rows are only ever inserted, in the same transaction as the change they describe. There
-- are no foreign keys: an entry never keeps a subject from being deleted (deleting one without
-- history stays possible), and it outlives the subject, naming it as it was. Nothing else changes.
CREATE TABLE agent_log (
    id TEXT PRIMARY KEY,
    application_id TEXT,
    resource_id TEXT,
    subject_name TEXT NOT NULL,
    agent TEXT NOT NULL CHECK (length(trim(agent)) BETWEEN 1 AND 120),
    action TEXT NOT NULL CHECK (action IN ('created', 'updated', 'deleted', 'check_saved',
        'check_paused', 'check_resumed', 'check_removed', 'check_run')),
    summary TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    occurred_at INTEGER NOT NULL,
    CHECK ((application_id IS NULL) <> (resource_id IS NULL))
);
CREATE INDEX agent_log_application_id ON agent_log(application_id, occurred_at);
CREATE INDEX agent_log_resource_id ON agent_log(resource_id, occurred_at);
