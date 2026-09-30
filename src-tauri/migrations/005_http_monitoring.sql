-- Periodic supervision of HTTP checks while DevHQ is open. Each check gains a supervision
-- state and interval. Only http_checks is rebuilt (SQLite cannot add a table CHECK with
-- ALTER, and nothing references this table); observations and events are untouched, so
-- their sources keep their v4 constraints. Rows keep their rowid, which orders the list.
-- Every existing check starts unsupervised: supervision only ever starts when asked.
CREATE TABLE http_checks_v5 (
    application_id TEXT UNIQUE REFERENCES applications(id) ON DELETE CASCADE,
    resource_id TEXT UNIQUE REFERENCES resources(id) ON DELETE CASCADE,
    url TEXT NOT NULL CHECK ((url LIKE 'http://%' OR url LIKE 'https://%') AND length(url) <= 2048),
    -- Redirects are followed, so the expected status is a final one, never a 3xx.
    expected_status INTEGER NOT NULL CHECK (expected_status BETWEEN 200 AND 599 AND expected_status NOT BETWEEN 300 AND 399),
    timeout_seconds INTEGER NOT NULL CHECK (timeout_seconds BETWEEN 1 AND 10),
    valid_for_minutes INTEGER NOT NULL CHECK (valid_for_minutes BETWEEN 1 AND 43200),
    updated_at INTEGER NOT NULL,
    -- off: runs only when asked. active: also runs every interval while DevHQ is open.
    -- paused: keeps its interval but does not run on its own until resumed.
    monitor TEXT NOT NULL DEFAULT 'off' CHECK (monitor IN ('off', 'active', 'paused')),
    monitor_interval_minutes INTEGER CHECK (monitor_interval_minutes BETWEEN 5 AND 60),
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    CHECK ((monitor = 'off') = (monitor_interval_minutes IS NULL)),
    -- A result must outlive the wait for the next one, so supervised evidence never lapses.
    CHECK (monitor_interval_minutes IS NULL OR valid_for_minutes > monitor_interval_minutes)
);
INSERT INTO http_checks_v5 (rowid, application_id, resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
SELECT rowid, application_id, resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at FROM http_checks;
DROP TABLE http_checks;
ALTER TABLE http_checks_v5 RENAME TO http_checks;
