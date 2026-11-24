-- Readings of GitHub repositories: what DevHQ read, when asked or when its window opened, of a
-- repository that a `repository` reference names on github.com (docs/leer-github.md). Like a reading
-- of a local copy (copy_readings, v12) it is evidence of its own, never health: what GitHub said at
-- one moment, with no validity, only ever inserted. It names its subject by ID with RESTRICT, so a
-- system with readings keeps its history, and it copies the reference and the repository's URL it
-- read, without foreign keys: references are replaced on every save of their system. `source` is
-- always 'github', `reader` names DevHQ's reader and the API version it asked for, and `agent` is the
-- agent that asked through the gate (NULL when the person asked or the window opened). Times are
-- milliseconds since the epoch, UTC.
--
-- The facts come only with outcome 'read': the default branch GitHub declared, the latest finished
-- run of each workflow on it (a JSON array of objects: workflow, conclusion, commit, finished time,
-- address) and the open pull requests (a JSON array of objects: number, title, draft, last change,
-- address), each list cut at 100 with `more` set when GitHub had more. Any other outcome says why in
-- `detail`: `unreadable` (no answer, an answer DevHQ refused, a limit) or `refused` (GitHub refused
-- the token or answered that it cannot see the repository). Without a token nothing is asked and
-- nothing is kept. The token is never here. Rust checks each element before writing. A new table: no row changes.
CREATE TABLE github_readings (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    reference_id TEXT NOT NULL,
    repository TEXT NOT NULL CHECK (length(repository) BETWEEN 20 AND 2048
        AND repository LIKE 'https://github.com/%'),
    source TEXT NOT NULL CHECK (source = 'github'),
    reader TEXT NOT NULL CHECK (length(reader) BETWEEN 1 AND 120),
    agent TEXT CHECK (length(trim(agent)) BETWEEN 1 AND 120),
    observed_at INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('read', 'unreadable', 'refused')),
    detail TEXT CHECK (length(detail) BETWEEN 1 AND 1000),
    default_branch TEXT CHECK (length(default_branch) BETWEEN 1 AND 255),
    workflows TEXT CHECK (workflows IS NULL OR (json_valid(workflows) AND json_type(workflows) = 'array')),
    workflows_more INTEGER CHECK (workflows_more IN (0, 1)),
    pull_requests TEXT CHECK (pull_requests IS NULL
        OR (json_valid(pull_requests) AND json_type(pull_requests) = 'array')),
    pull_requests_more INTEGER CHECK (pull_requests_more IN (0, 1)),
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    -- A repository that was not read says why, and nothing else.
    CHECK (outcome = 'read' OR (detail IS NOT NULL AND default_branch IS NULL AND workflows IS NULL
        AND workflows_more IS NULL AND pull_requests IS NULL AND pull_requests_more IS NULL)),
    -- A repository that was read has every fact, and no reason.
    CHECK (outcome <> 'read' OR (detail IS NULL AND default_branch IS NOT NULL
        AND workflows IS NOT NULL AND workflows_more IS NOT NULL
        AND pull_requests IS NOT NULL AND pull_requests_more IS NOT NULL))
);
-- The latest reading of each declared repository, which every read of the inventory carries.
CREATE INDEX github_readings_reference_id ON github_readings(reference_id, observed_at);
-- A system's readings, newest first, a page at a time; and whether it has any (history).
CREATE INDEX github_readings_application_id ON github_readings(application_id, observed_at);
CREATE INDEX github_readings_resource_id ON github_readings(resource_id, observed_at);
