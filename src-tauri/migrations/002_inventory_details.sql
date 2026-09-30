ALTER TABLE applications ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE applications ADD COLUMN version TEXT;
ALTER TABLE resources ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE resources ADD COLUMN operating_system TEXT NOT NULL DEFAULT '';
