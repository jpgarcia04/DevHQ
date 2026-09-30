-- Operational records reference exactly one application or resource through real
-- foreign keys. History uses RESTRICT: removing a subject must never silently erase
-- or orphan its evidence. Times are Unix epoch milliseconds (UTC).
CREATE TABLE observations (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    result TEXT NOT NULL CHECK (result IN ('healthy', 'degraded', 'unhealthy')),
    summary TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    source TEXT NOT NULL CHECK (source IN ('manual')),
    observed_at INTEGER NOT NULL,
    valid_until INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    CHECK (valid_until > observed_at)
);
CREATE INDEX observations_application_id ON observations(application_id, observed_at);
CREATE INDEX observations_resource_id ON observations(resource_id, observed_at);

CREATE TABLE incidents (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description TEXT NOT NULL DEFAULT '',
    severity TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    status TEXT NOT NULL CHECK (status IN ('open', 'investigating', 'resolved')),
    source TEXT NOT NULL CHECK (source IN ('manual')),
    opened_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    resolved_at INTEGER,
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    CHECK ((status = 'resolved') = (resolved_at IS NOT NULL))
);
CREATE INDEX incidents_application_id ON incidents(application_id);
CREATE INDEX incidents_resource_id ON incidents(resource_id);

-- The timeline. Observations and incident changes write their event in the same
-- transaction; the remaining kinds are events logged by hand.
CREATE TABLE events (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('observation', 'incident', 'deployment', 'backup', 'maintenance', 'note')),
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'warning', 'failure', 'info')),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    detail TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL CHECK (source IN ('manual')),
    observation_id TEXT REFERENCES observations(id) ON DELETE RESTRICT,
    incident_id TEXT REFERENCES incidents(id) ON DELETE RESTRICT,
    occurred_at INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    CHECK ((kind = 'observation') = (observation_id IS NOT NULL)),
    CHECK ((kind = 'incident') = (incident_id IS NOT NULL))
);
CREATE INDEX events_application_id ON events(application_id, occurred_at);
CREATE INDEX events_resource_id ON events(resource_id, occurred_at);
CREATE INDEX events_observation_id ON events(observation_id);
CREATE INDEX events_incident_id ON events(incident_id);
