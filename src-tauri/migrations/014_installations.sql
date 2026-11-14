-- A sixth kind of reference, 'installation': the absolute path of a system's installed executable on
-- a workstation, declared like a local copy and the only thing DevHQ reads of it (see
-- installations.rs). SQLite cannot change a CHECK in place, so system_references is rebuilt as v7
-- and v13 rebuilt their tables: every row keeps its rowid, its ID and its values, and the three
-- indexes are made again under their names. Nothing refers to this table (a reading copies its
-- reference's ID, without a foreign key), so no other table is touched.
CREATE TABLE system_references_v14 (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE CASCADE,
    resource_id TEXT REFERENCES resources(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('repository', 'local_copy', 'documentation', 'logs', 'dashboard', 'installation')),
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
    -- A repository and a dashboard are web addresses; a local copy and an installation are paths.
    CHECK (kind NOT IN ('repository', 'dashboard') OR machine_id IS NULL),
    CHECK (kind NOT IN ('local_copy', 'installation') OR machine_id IS NOT NULL)
);
INSERT INTO system_references_v14 (rowid, id, application_id, resource_id, kind, target, label, machine_id)
    SELECT rowid, id, application_id, resource_id, kind, target, label, machine_id FROM system_references;
DROP TABLE system_references;
ALTER TABLE system_references_v14 RENAME TO system_references;
CREATE INDEX system_references_application_id ON system_references(application_id);
CREATE INDEX system_references_resource_id ON system_references(resource_id);
CREATE INDEX system_references_machine_id ON system_references(machine_id);

-- Readings of installations: what DevHQ read, when asked, in the executable file that an
-- `installation` reference declares on this workstation. Like a reading of a local copy
-- (copy_readings, v12) it is evidence of its own, never health: a fact about a file at one moment,
-- with no validity, only ever inserted. It names its subject by ID with RESTRICT, so a system with
-- readings keeps its history, and it copies the reference, the machine and the path it read,
-- without foreign keys. `source` is always 'read', `reader` names DevHQ's reader and its version,
-- and `agent` is the agent that asked through the gate (NULL when the person asked in the window).
-- Times are milliseconds since the epoch, UTC.
--
-- `stored_at` is where the system really kept the file when that was another path than the one
-- declared (a link, a junction, a packaged app's redirection of AppData). The facts come only with
-- outcome 'read': the file's SHA-256, size and modification time; what its version resource
-- declares, as the file says it, or why that is unknown; and the processes whose executable was
-- that file, counted with the ones the system did not let DevHQ look at, or why they are unknown.
-- Never a process identifier or a command line. Any other outcome says why in `detail`. Rust checks
-- the rest before writing. A new table: no row changes.
CREATE TABLE installation_readings (
    id TEXT PRIMARY KEY,
    application_id TEXT REFERENCES applications(id) ON DELETE RESTRICT,
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT,
    reference_id TEXT NOT NULL,
    machine_id TEXT NOT NULL,
    path TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 2048),
    source TEXT NOT NULL CHECK (source = 'read'),
    reader TEXT NOT NULL CHECK (length(reader) BETWEEN 1 AND 120),
    agent TEXT CHECK (length(trim(agent)) BETWEEN 1 AND 120),
    observed_at INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('read', 'missing', 'unreadable', 'not_an_executable')),
    detail TEXT CHECK (length(detail) BETWEEN 1 AND 1000),
    stored_at TEXT CHECK (length(stored_at) BETWEEN 1 AND 4096),
    sha256 TEXT CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    size_bytes INTEGER CHECK (size_bytes >= 0),
    modified_at INTEGER,
    product_version TEXT CHECK (length(product_version) BETWEEN 1 AND 120),
    file_version TEXT CHECK (length(file_version) BETWEEN 1 AND 120),
    product_name TEXT CHECK (length(product_name) BETWEEN 1 AND 120),
    version_unknown TEXT CHECK (length(version_unknown) BETWEEN 1 AND 1000),
    processes INTEGER CHECK (processes >= 0),
    processes_unexamined INTEGER CHECK (processes_unexamined >= 0),
    processes_unknown TEXT CHECK (length(processes_unknown) BETWEEN 1 AND 1000),
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    -- A file that was not read says why, and nothing else but where the system keeps it; nothing
    -- is at a path where nothing was found.
    CHECK (outcome = 'read' OR (detail IS NOT NULL AND sha256 IS NULL AND size_bytes IS NULL
        AND modified_at IS NULL AND product_version IS NULL AND file_version IS NULL
        AND product_name IS NULL AND version_unknown IS NULL AND processes IS NULL
        AND processes_unexamined IS NULL AND processes_unknown IS NULL)),
    CHECK (outcome <> 'missing' OR stored_at IS NULL),
    -- A file that was read has its content's hash and size, and no reason.
    CHECK (outcome <> 'read' OR (detail IS NULL AND sha256 IS NOT NULL AND size_bytes IS NOT NULL)),
    -- What its version resource declares, or why that is unknown; never both, never neither.
    CHECK (outcome <> 'read' OR ((product_version IS NOT NULL OR file_version IS NOT NULL
        OR product_name IS NOT NULL) <> (version_unknown IS NOT NULL))),
    -- Its processes are counted, or unknown with the reason; never both, never half.
    CHECK (outcome <> 'read' OR ((processes IS NOT NULL) <> (processes_unknown IS NOT NULL))),
    CHECK ((processes IS NULL) = (processes_unexamined IS NULL))
);
-- The latest readings of each declared installation, which every read of the inventory carries.
CREATE INDEX installation_readings_reference_id ON installation_readings(reference_id, observed_at);
-- A system's readings, newest first, a page at a time; and whether it has any (history).
CREATE INDEX installation_readings_application_id ON installation_readings(application_id, observed_at);
CREATE INDEX installation_readings_resource_id ON installation_readings(resource_id, observed_at);
