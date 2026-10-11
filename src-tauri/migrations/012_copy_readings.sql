-- Readings of local copies: what DevHQ read, when asked, in the working copy that a `local_copy`
-- reference declares on this workstation (see local_copies.rs). A reading is evidence of its own,
-- never health: it is not healthy, degraded or unhealthy, and it enters neither the attention list
-- nor health coverage, so it is not one more source of `observations`. Like every record, it is
-- only ever inserted, and it names its subject by ID with RESTRICT, so a system with readings keeps
-- its history and can only be archived.
--
-- It copies the reference it read (`reference_id`), the machine (`machine_id`) and the path, without
-- foreign keys, as an observation copies the URL its check requested: references are replaced on
-- every save of their system, a copy can be moved or removed, and a machine deleted once it holds no
-- path, without touching what was read. `source` is always 'read' and `reader` names the library and
-- its version. `agent` is the agent that asked through the gate; NULL when the person asked in the
-- window. Times are milliseconds since the epoch, UTC.
--
-- The facts come only with outcome 'read'; any other outcome says why in `detail`. Unpublished
-- commits are counted up to 1000; 1001 means more. The working tree is either counted (all three)
-- or unknown, with the reason. Rust checks the rest before writing. A new table: no row changes.
CREATE TABLE copy_readings (
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
    outcome TEXT NOT NULL CHECK (outcome IN ('read', 'missing', 'not_a_copy', 'unreadable')),
    detail TEXT CHECK (length(detail) BETWEEN 1 AND 1000),
    branch TEXT CHECK (length(branch) BETWEEN 1 AND 255),
    head TEXT CHECK (length(head) = 40 AND head NOT GLOB '*[^0-9a-f]*'),
    last_commit_at INTEGER,
    unpublished_on_head INTEGER CHECK (unpublished_on_head BETWEEN 0 AND 1001),
    unpublished_elsewhere INTEGER CHECK (unpublished_elsewhere BETWEEN 0 AND 1001),
    stashes INTEGER CHECK (stashes >= 0),
    synced_at INTEGER,
    remotes TEXT CHECK (remotes IS NULL OR (json_valid(remotes) AND json_type(remotes) = 'array')),
    operation TEXT CHECK (operation IN ('merge', 'rebase', 'cherry_pick', 'revert', 'bisect', 'apply_mailbox')),
    changed INTEGER CHECK (changed >= 0),
    untracked INTEGER CHECK (untracked >= 0),
    unverifiable INTEGER CHECK (unverifiable >= 0),
    worktree_unknown TEXT CHECK (length(worktree_unknown) BETWEEN 1 AND 1000),
    CHECK ((application_id IS NULL) <> (resource_id IS NULL)),
    -- A copy that was not read says why, and nothing else.
    CHECK (outcome = 'read' OR (detail IS NOT NULL AND branch IS NULL AND head IS NULL
        AND last_commit_at IS NULL AND unpublished_on_head IS NULL AND unpublished_elsewhere IS NULL
        AND stashes IS NULL AND synced_at IS NULL AND remotes IS NULL AND operation IS NULL
        AND changed IS NULL AND untracked IS NULL AND unverifiable IS NULL AND worktree_unknown IS NULL)),
    -- A copy that was read has every fact but the optional ones, and no reason.
    CHECK (outcome <> 'read' OR (detail IS NULL AND unpublished_on_head IS NOT NULL
        AND unpublished_elsewhere IS NOT NULL AND stashes IS NOT NULL AND remotes IS NOT NULL)),
    -- Its working tree is counted, or unknown with the reason; never both, never half.
    CHECK (outcome <> 'read' OR ((changed IS NOT NULL) <> (worktree_unknown IS NOT NULL))),
    CHECK ((changed IS NULL) = (untracked IS NULL) AND (untracked IS NULL) = (unverifiable IS NULL)),
    -- No commit on HEAD, nothing unpublished on it.
    CHECK (head IS NOT NULL OR unpublished_on_head IS NULL OR unpublished_on_head = 0)
);
-- The latest reading of each declared copy, which every read of the inventory carries.
CREATE INDEX copy_readings_reference_id ON copy_readings(reference_id, observed_at);
-- A system's readings, newest first, a page at a time; and whether it has any (history).
CREATE INDEX copy_readings_application_id ON copy_readings(application_id, observed_at);
CREATE INDEX copy_readings_resource_id ON copy_readings(resource_id, observed_at);
