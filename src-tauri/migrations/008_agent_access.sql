-- Whether agents may use the agent gate (`devhq --mcp`): 'off', 'read' or 'write'. The person
-- changes it only from the window's Settings; the gate reads it on every call, so a change applies
-- at once. At most one row; no row is 'off', so an upgraded inventory starts closed to agents and
-- opening it is always an explicit decision. Nothing else changes.
CREATE TABLE agent_access (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    mode TEXT NOT NULL CHECK (mode IN ('off', 'read', 'write')),
    changed_at INTEGER NOT NULL
);
