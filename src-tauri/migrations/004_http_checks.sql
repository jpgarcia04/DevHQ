-- On-demand HTTP checks. Observations gain a second source, 'http', whose technical
-- evidence lives in http_* columns that manual observations never use. SQLite cannot
-- change a CHECK in place, so observations and events are rebuilt: rows, rowids (the
-- tie-break order when times are equal), IDs, foreign keys and indexes are carried over.
-- The migration runs with foreign keys off and is only committed after
-- PRAGMA foreign_key_check reports no broken relationship.
CREATE TABLE observations_v4 (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    result TEXT NOT NULL CHECK (result IN ('healthy', 'degraded', 'unhealthy')),
    summary TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    source TEXT NOT NULL CHECK (source IN ('manual', 'http')),
    observed_at INTEGER NOT NULL,
    valid_until INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    -- What an HTTP check requested, what came back and how long it took. The URL and
    -- expected status are copied, so later configuration changes never rewrite evidence.
    -- Response bodies and headers are never stored. Redirects are a JSON array of hops.
    http_url TEXT,
    http_expected_status INTEGER,
    http_status INTEGER,
    http_latency_ms INTEGER CHECK (http_latency_ms >= 0),
    http_error TEXT,
    http_redirects TEXT,
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    CHECK (valid_until > observed_at),
    CHECK ((source = 'http') = (http_url IS NOT NULL)),
    CHECK ((source = 'http') = (http_expected_status IS NOT NULL)),
    CHECK ((source = 'http') = (http_latency_ms IS NOT NULL)),
    CHECK ((source = 'http') = (http_redirects IS NOT NULL)),
    CHECK (source = 'http' OR (http_status IS NULL AND http_error IS NULL)),
    CHECK (source <> 'http' OR http_status IS NOT NULL OR http_error IS NOT NULL),
    -- A check is healthy exactly when the expected status arrived without error.
    CHECK (source <> 'http' OR result = CASE
        WHEN http_error IS NULL AND http_status = http_expected_status THEN 'healthy'
        ELSE 'unhealthy' END)
);
INSERT INTO observations_v4 (rowid, id, application_id, resource_id, result, summary, source, observed_at, valid_until, recorded_at)
SELECT rowid, id, application_id, resource_id, result, summary, source, observed_at, valid_until, recorded_at FROM observations;

CREATE TABLE events_v4 (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('observation', 'incident', 'deployment', 'backup', 'maintenance', 'note')),
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'warning', 'failure', 'info')),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    detail TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL CHECK (source IN ('manual', 'http')),
    observation_id TEXT REFERENCES observations(id) ON DELETE RESTRICT,
    incident_id TEXT REFERENCES incidents(id) ON DELETE RESTRICT,
    occurred_at INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    CHECK ((kind = 'observation') = (observation_id IS NOT NULL)),
    CHECK ((kind = 'incident') = (incident_id IS NOT NULL)),
    -- Only an HTTP check's own observation event can come from it.
    CHECK (source = 'manual' OR kind = 'observation')
);
INSERT INTO events_v4 (rowid, id, application_id, resource_id, kind, outcome, title, detail, source, observation_id, incident_id, occurred_at, recorded_at)
SELECT rowid, id, application_id, resource_id, kind, outcome, title, detail, source, observation_id, incident_id, occurred_at, recorded_at FROM events;

DROP TABLE events;
DROP TABLE observations;
ALTER TABLE observations_v4 RENAME TO observations;
ALTER TABLE events_v4 RENAME TO events;
CREATE INDEX observations_application_id ON observations(application_id, observed_at);
CREATE INDEX observations_resource_id ON observations(resource_id, observed_at);
CREATE INDEX events_application_id ON events(application_id, occurred_at);
CREATE INDEX events_resource_id ON events(resource_id, occurred_at);
CREATE INDEX events_observation_id ON events(observation_id);
CREATE INDEX events_incident_id ON events(incident_id);

-- At most one HTTP check per application or resource. It is configuration, not history:
-- it goes with its subject (CASCADE), while the observations it produced keep their own
-- copy of what was checked and still prevent deleting that subject (RESTRICT).
CREATE TABLE http_checks (
    application_id TEXT UNIQUE REFERENCES applications(id) ON DELETE CASCADE,
    resource_id TEXT UNIQUE REFERENCES resources(id) ON DELETE CASCADE,
    url TEXT NOT NULL CHECK ((url LIKE 'http://%' OR url LIKE 'https://%') AND length(url) <= 2048),
    -- Redirects are followed, so the expected status is a final one, never a 3xx.
    expected_status INTEGER NOT NULL CHECK (expected_status BETWEEN 200 AND 599 AND expected_status NOT BETWEEN 300 AND 399),
    timeout_seconds INTEGER NOT NULL CHECK (timeout_seconds BETWEEN 1 AND 10),
    valid_for_minutes INTEGER NOT NULL CHECK (valid_for_minutes BETWEEN 1 AND 43200),
    updated_at INTEGER NOT NULL,
    CHECK ((application_id IS NULL) <> (resource_id IS NULL))
);
