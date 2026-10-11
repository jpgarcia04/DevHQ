-- References: where each system is in the world. Its repository, its local copies on each
-- workstation, its documentation, its logs and its dashboard. They are inventory that the person
-- or an agent declares, not evidence: no source (their trace is the change log, where they are a
-- field of their system's entries), and DevHQ never opens or checks them. Named system_references
-- because REFERENCES is a keyword of SQL.
--
-- Each one hangs from one application or one resource and goes with it (CASCADE, like
-- http_checks). Its target is an http(s) URL or an absolute path, as it was typed; a path is on a
-- machine, a resource, which cannot be deleted while it holds a path (RESTRICT): that would change
-- another system's configuration without an entry in the change log. SQLite applies RESTRICT at
-- once, before any cascade, so deleting a resource removes its own references first. The CHECKs
-- hold what SQL can say; Rust checks the rest before writing (a well-formed URL without a user name
-- or password, and that a local copy is on a workstation). A new table: no row changes.
CREATE TABLE system_references (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE CASCADE,
    resource_id TEXT REFERENCES resources(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('repository', 'local_copy', 'documentation', 'logs', 'dashboard')),
    target TEXT NOT NULL CHECK (length(target) BETWEEN 1 AND 2048 AND target = trim(target)),
    -- A name for people that tells two references of one kind apart; never a locator.
    label TEXT CHECK (length(label) BETWEEN 1 AND 120 AND label = trim(label)),
    machine_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    -- A URL is on no machine. Anything else is an absolute path (C:\…, \\server\share\… or /…),
    -- which is always on one.
    CHECK ((target LIKE 'http://%' OR target LIKE 'https://%') = (machine_id IS NULL)),
    CHECK (machine_id IS NULL OR substr(target, 1, 1) = '/' OR substr(target, 1, 2) = '\\'
        OR target GLOB '[A-Za-z]:[\/]*'),
    -- A repository and a dashboard are web addresses; a local copy is a path.
    CHECK (kind NOT IN ('repository', 'dashboard') OR machine_id IS NULL),
    CHECK (kind <> 'local_copy' OR machine_id IS NOT NULL)
);
CREATE INDEX system_references_application_id ON system_references(application_id);
CREATE INDEX system_references_resource_id ON system_references(resource_id);
CREATE INDEX system_references_machine_id ON system_references(machine_id);
