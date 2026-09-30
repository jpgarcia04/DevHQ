CREATE TABLE applications (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('development', 'testing', 'production', 'archived'))
);
CREATE TABLE resources (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    kind TEXT NOT NULL CHECK (kind IN ('server', 'workstation'))
);
CREATE TABLE components (
    id TEXT PRIMARY KEY,
    application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    kind TEXT NOT NULL CHECK (kind IN ('backend', 'database', 'docker', 'frontend')),
    resource_id TEXT REFERENCES resources(id) ON DELETE RESTRICT
);
CREATE INDEX components_application_id ON components(application_id);
CREATE INDEX components_resource_id ON components(resource_id);
