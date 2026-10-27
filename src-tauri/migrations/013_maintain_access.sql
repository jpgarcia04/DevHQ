-- A fourth access mode for the agent gate, 'maintain', between 'read' and 'write': agents may also
-- add and edit applications and resources, with their components and references, and nothing
-- else. Like the others, the person chooses it only from the window's Settings and the gate reads it
-- on every call. SQLite cannot change a CHECK in place, so agent_access is rebuilt as in v7: its one
-- row, when there is one, keeps its rowid and values, so every inventory keeps the access the person
-- chose, and an inventory without a row stays closed to agents.
CREATE TABLE agent_access_v13 (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    mode TEXT NOT NULL CHECK (mode IN ('off', 'read', 'maintain', 'write')),
    changed_at INTEGER NOT NULL
);
INSERT INTO agent_access_v13 (id, mode, changed_at) SELECT id, mode, changed_at FROM agent_access;
DROP TABLE agent_access;
ALTER TABLE agent_access_v13 RENAME TO agent_access;

-- An agent's edit names the state it was based on: the last entry of its system's change log by
-- insertion order (rowid), which the gate looks up with SQLite's write lock held, and gives with
-- every system it lists. The indexes of v10 keep each subject's entries by time, so finding the last
-- one by rowid read and sorted every entry about it. These keep them by rowid (an index's last
-- column), so that it is one step however long the log grows. Only indexes: no row changes.
CREATE INDEX change_log_application_latest ON change_log(application_id);
CREATE INDEX change_log_resource_latest ON change_log(resource_id);
