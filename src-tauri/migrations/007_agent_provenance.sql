-- A third source, 'agent': records an agent (such as Claude Code) writes through DevHQ's agent
-- gate when the person asks it to. They carry, in `agent`, the name the agent's client declared
-- for itself; every other source leaves it empty. Like manual entries, they are reported rather
-- than measured, so the partial indexes that keep them loaded in full now cover both.
-- SQLite cannot change a CHECK in place, so observations, incidents and events are rebuilt as in
-- v4: rows, rowids (the tie-break order when times are equal), IDs, foreign keys and indexes are
-- carried over, and every existing row keeps its source with no agent. The migration runs with
-- foreign keys off and is only committed after PRAGMA foreign_key_check reports no broken
-- relationship.
CREATE TABLE observations_v7 (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    result TEXT NOT NULL CHECK (result IN ('healthy', 'degraded', 'unhealthy')),
    summary TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    source TEXT NOT NULL CHECK (source IN ('manual', 'http', 'agent')),
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
    agent TEXT CHECK (length(trim(agent)) BETWEEN 1 AND 120),
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
        ELSE 'unhealthy' END),
    CHECK ((source = 'agent') = (agent IS NOT NULL))
);
INSERT INTO observations_v7 (rowid, id, application_id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                             http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
SELECT rowid, id, application_id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
       http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects FROM observations;

CREATE TABLE incidents_v7 (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description TEXT NOT NULL DEFAULT '',
    severity TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    status TEXT NOT NULL CHECK (status IN ('open', 'investigating', 'resolved')),
    -- Who opened it. Each later change of status is an event with its own source.
    source TEXT NOT NULL CHECK (source IN ('manual', 'agent')),
    opened_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    resolved_at INTEGER,
    agent TEXT CHECK (length(trim(agent)) BETWEEN 1 AND 120),
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    CHECK ((status = 'resolved') = (resolved_at IS NOT NULL)),
    CHECK ((source = 'agent') = (agent IS NOT NULL))
);
INSERT INTO incidents_v7 (rowid, id, application_id, resource_id, title, description, severity, status, source, opened_at, updated_at, resolved_at)
SELECT rowid, id, application_id, resource_id, title, description, severity, status, source, opened_at, updated_at, resolved_at FROM incidents;

CREATE TABLE events_v7 (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('observation', 'incident', 'deployment', 'backup', 'maintenance', 'note')),
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'warning', 'failure', 'info')),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    detail TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL CHECK (source IN ('manual', 'http', 'agent')),
    observation_id TEXT REFERENCES observations(id) ON DELETE RESTRICT,
    incident_id TEXT REFERENCES incidents(id) ON DELETE RESTRICT,
    occurred_at INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    agent TEXT CHECK (length(trim(agent)) BETWEEN 1 AND 120),
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    CHECK ((kind = 'observation') = (observation_id IS NOT NULL)),
    CHECK ((kind = 'incident') = (incident_id IS NOT NULL)),
    -- Only an HTTP check's own observation event can come from it.
    CHECK (source <> 'http' OR kind = 'observation'),
    CHECK ((source = 'agent') = (agent IS NOT NULL))
);
INSERT INTO events_v7 (rowid, id, application_id, resource_id, kind, outcome, title, detail, source, observation_id, incident_id, occurred_at, recorded_at)
SELECT rowid, id, application_id, resource_id, kind, outcome, title, detail, source, observation_id, incident_id, occurred_at, recorded_at FROM events;

DROP TABLE events;
DROP TABLE incidents;
DROP TABLE observations;
ALTER TABLE observations_v7 RENAME TO observations;
ALTER TABLE incidents_v7 RENAME TO incidents;
ALTER TABLE events_v7 RENAME TO events;
CREATE INDEX observations_application_id ON observations(application_id, observed_at);
CREATE INDEX observations_resource_id ON observations(resource_id, observed_at);
CREATE INDEX incidents_application_id ON incidents(application_id);
CREATE INDEX incidents_resource_id ON incidents(resource_id);
CREATE INDEX events_application_id ON events(application_id, occurred_at);
CREATE INDEX events_resource_id ON events(resource_id, occurred_at);
CREATE INDEX events_observation_id ON events(observation_id);
CREATE INDEX events_incident_id ON events(incident_id);
-- Reported records, by a person or an agent, are loaded in full (see records::load). The
-- queries repeat this exact condition, so SQLite can tell these indexes serve them.
CREATE INDEX observations_reported ON observations(observed_at) WHERE source IN ('manual', 'agent');
CREATE INDEX events_reported ON events(occurred_at) WHERE source IN ('manual', 'agent');
