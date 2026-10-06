-- One change log for every significant change to the configuration (applications, resources,
-- components and HTTP checks), whoever makes it: the person in the window (`manual`) or an agent
-- through the agent gate (`agent`, with the name its client declared), the same vocabulary as the
-- records' `source`. It grows from v9's agent_log, which only held what agents changed: every row
-- is carried over, with its rowid (the tie-break order at equal times), as an agent's change.
-- New entries also keep what changed as structured JSON (`changes`: an array of field, value
-- before and value after), so that the log can say how something came to be as it is and not
-- only who touched it; the rows carried over have only their summary. Entries are only ever
-- inserted, in the same transaction as the change. No foreign keys: an entry never keeps a
-- subject from being deleted, and outlives it with the name it had. Nothing else changes.
CREATE TABLE change_log (
    id TEXT PRIMARY KEY,
    application_id TEXT,
    resource_id TEXT,
    subject_name TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('manual', 'agent')),
    agent TEXT CHECK (length(trim(agent)) BETWEEN 1 AND 120),
    action TEXT NOT NULL CHECK (action IN ('created', 'updated', 'deleted', 'check_saved',
        'check_paused', 'check_resumed', 'check_removed', 'check_run')),
    summary TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    changes TEXT CHECK (changes IS NULL OR (json_valid(changes) AND json_type(changes) = 'array')),
    occurred_at INTEGER NOT NULL,
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    CHECK ((source = 'agent') = (agent IS NOT NULL))
);
INSERT INTO change_log (rowid, id, application_id, resource_id, subject_name, source, agent, action, summary, changes, occurred_at)
SELECT rowid, id, application_id, resource_id, subject_name, 'agent', agent, action, summary, NULL, occurred_at FROM agent_log;
DROP TABLE agent_log;
CREATE INDEX change_log_application_id ON change_log(application_id, occurred_at);
CREATE INDEX change_log_resource_id ON change_log(resource_id, occurred_at);

-- Until v10 the person's changes left no trace. When this upgrade finds an inventory already
-- holding applications or resources, the log is complete only from this moment on, and the
-- window says so; an empty inventory (or a new one) has nothing earlier to miss, so no row.
CREATE TABLE change_log_start (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    started_at INTEGER NOT NULL
);
INSERT INTO change_log_start (id, started_at)
SELECT 1, CAST(unixepoch('subsec') * 1000 AS INTEGER)
WHERE EXISTS (SELECT 1 FROM applications) OR EXISTS (SELECT 1 FROM resources);
