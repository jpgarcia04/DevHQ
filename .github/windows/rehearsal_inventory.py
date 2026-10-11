"""The SQLite side of the upgrade rehearsal (.github/workflows/upgrade-rehearsal.yml), on the
throwaway Windows runner's synthetic inventory only. Never point it at a real inventory.

snapshot   Copies the inventory, with its -wal and -shm when they exist, into a temporary folder
           and describes the copy, so the file under test is never opened: schema version,
           integrity, foreign keys, and every table's schema, row count and a SHA-256 of its rows
           (order-independent). Two snapshots that agree mean no row was lost, added or changed.
compare    Compares two snapshots table by table and exits 1 on any difference.
set-access Sets the agent gate's access mode with the very statement DevHQ's Settings runs
           (src-tauri/src/access.rs), after checking the schema version and that table's shape.
           Only for the rehearsal: DevHQ offers no other way to set it without driving its window,
           and the installed build has no DevTools port to drive. The window and the gate must be
           closed; it refuses when a write-ahead log is present.
"""

import argparse
import hashlib
import json
import os
import shutil
import sqlite3
import sys
import tempfile
import time

SCHEMA_VERSION = 12
# agent_access as migration 008 creates it: PRAGMA table_info rows (cid, name, type, notnull,
# default, pk).
AGENT_ACCESS_COLUMNS = [
    (0, "id", "INTEGER", 0, None, 1),
    (1, "mode", "TEXT", 1, None, 0),
    (2, "changed_at", "INTEGER", 1, None, 0),
]
AGENT_ACCESS_CHECK = "CHECK (mode IN ('off', 'read', 'write'))"


def canonical(value):
    if isinstance(value, bytes):
        return {"blob": value.hex()}
    return value


def describe(path):
    if not os.path.isfile(path):
        raise SystemExit(f"No inventory at {path}")
    with tempfile.TemporaryDirectory(prefix="devhq-rehearsal-") as folder:
        copy = os.path.join(folder, "inventory.sqlite3")
        shutil.copyfile(path, copy)
        companions = []
        for suffix in ("-wal", "-shm"):
            if os.path.exists(path + suffix):
                shutil.copyfile(path + suffix, copy + suffix)
                companions.append({"file": suffix, "bytes": os.path.getsize(path + suffix)})
        db = sqlite3.connect(copy)
        try:
            tables = {}
            names = [
                row[0]
                for row in db.execute(
                    "SELECT name FROM sqlite_master WHERE type = 'table' "
                    "AND name NOT LIKE 'sqlite_%' ORDER BY name"
                )
            ]
            for name in names:
                sql = db.execute(
                    "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?", (name,)
                ).fetchone()[0]
                rows = sorted(
                    json.dumps([canonical(value) for value in row], ensure_ascii=False)
                    for row in db.execute(f'SELECT * FROM "{name}"')
                )
                tables[name] = {
                    "sqlSha256": hashlib.sha256(sql.encode()).hexdigest(),
                    "rows": len(rows),
                    "rowsSha256": hashlib.sha256("\n".join(rows).encode()).hexdigest(),
                }
            indexes = sorted(
                f"{row[0]}: {row[1]}"
                for row in db.execute(
                    "SELECT name, sql FROM sqlite_master WHERE type IN ('index', 'trigger', 'view') "
                    "AND sql IS NOT NULL"
                )
            )
            access = [
                list(row) for row in db.execute("SELECT id, mode, changed_at FROM agent_access")
            ]
            return {
                "source": path,
                "companions": companions,
                "userVersion": db.execute("PRAGMA user_version").fetchone()[0],
                "journalMode": db.execute("PRAGMA journal_mode").fetchone()[0],
                "integrity": [row[0] for row in db.execute("PRAGMA integrity_check")],
                "foreignKeyViolations": len(db.execute("PRAGMA foreign_key_check").fetchall()),
                "otherSchemaSha256": hashlib.sha256("\n".join(indexes).encode()).hexdigest(),
                "agentAccess": access,
                "tables": tables,
            }
        finally:
            db.close()


def differences(before, after, allowed):
    found = []
    for key in ("userVersion", "otherSchemaSha256"):
        if before[key] != after[key]:
            found.append(f"{key}: {before[key]} -> {after[key]}")
    for snapshot in (before, after):
        if snapshot["integrity"] != ["ok"] or snapshot["foreignKeyViolations"]:
            found.append(
                f"{snapshot['source']}: integrity {snapshot['integrity']}, "
                f"{snapshot['foreignKeyViolations']} foreign key violations"
            )
    for name in sorted(set(before["tables"]) | set(after["tables"])):
        if name in allowed:
            continue
        old, new = before["tables"].get(name), after["tables"].get(name)
        if old is None or new is None:
            found.append(f"table {name}: {'added' if old is None else 'removed'}")
        elif old != new:
            found.append(f"table {name}: {old['rows']} rows -> {new['rows']} rows, contents differ")
    return found


def set_access(path, mode):
    if mode not in ("off", "read", "write"):
        raise SystemExit(f"Unknown mode {mode}")
    if os.path.exists(path + "-wal"):
        raise SystemExit(f"{path}-wal exists: DevHQ may be open on this inventory.")
    db = sqlite3.connect(path, isolation_level=None)
    try:
        version = db.execute("PRAGMA user_version").fetchone()[0]
        if version != SCHEMA_VERSION:
            raise SystemExit(f"Schema version {version}, not {SCHEMA_VERSION}")
        columns = [tuple(row) for row in db.execute("PRAGMA table_info(agent_access)")]
        if columns != AGENT_ACCESS_COLUMNS:
            raise SystemExit(f"agent_access is not migration 008's table: {columns}")
        sql = db.execute(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'agent_access'"
        ).fetchone()[0]
        if AGENT_ACCESS_CHECK not in sql:
            raise SystemExit(f"agent_access lacks {AGENT_ACCESS_CHECK}: {sql}")
        db.execute("BEGIN IMMEDIATE")
        # src-tauri/src/access.rs, set_agent_access.
        db.execute(
            "INSERT INTO agent_access (id, mode, changed_at) VALUES (1, ?1, ?2) "
            "ON CONFLICT (id) DO UPDATE SET mode = excluded.mode, changed_at = excluded.changed_at",
            (mode, int(time.time() * 1000)),
        )
        db.execute("COMMIT")
        rows = db.execute("SELECT id, mode, changed_at FROM agent_access").fetchall()
    finally:
        db.close()
    if rows != [(1, mode, rows[0][2])]:
        raise SystemExit(f"agent_access holds {rows}")
    left = os.path.exists(path + "-wal")
    print(json.dumps({"agentAccess": [list(row) for row in rows], "walLeft": left}))
    if left:
        raise SystemExit("The write-ahead log stayed after the connection closed.")


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    snapshot = commands.add_parser("snapshot")
    snapshot.add_argument("--database", required=True)
    snapshot.add_argument("--out", required=True)
    compare = commands.add_parser("compare")
    compare.add_argument("before")
    compare.add_argument("after")
    compare.add_argument("--allow", action="append", default=[])
    access = commands.add_parser("set-access")
    access.add_argument("--database", required=True)
    access.add_argument("--mode", required=True)
    arguments = parser.parse_args()

    if arguments.command == "snapshot":
        described = describe(arguments.database)
        with open(arguments.out, "w", encoding="utf-8") as out:
            json.dump(described, out, indent=2, ensure_ascii=False)
        print(
            json.dumps(
                {
                    "userVersion": described["userVersion"],
                    "integrity": described["integrity"],
                    "foreignKeyViolations": described["foreignKeyViolations"],
                    "companions": described["companions"],
                    "agentAccess": described["agentAccess"],
                    "rows": {name: table["rows"] for name, table in described["tables"].items()},
                }
            )
        )
    elif arguments.command == "compare":
        with open(arguments.before, encoding="utf-8") as file:
            before = json.load(file)
        with open(arguments.after, encoding="utf-8") as file:
            after = json.load(file)
        found = differences(before, after, set(arguments.allow))
        for line in found:
            print(f"DIFFERENT {line}")
        if found:
            sys.exit(1)
        print(f"identical: {len(after['tables'])} tables, schema version {after['userVersion']}")
    else:
        set_access(arguments.database, arguments.mode)


if __name__ == "__main__":
    main()
