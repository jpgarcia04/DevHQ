-- A snapshot carries every manual record but only the latest automated results of each
-- application or resource (see records::load): supervision can add one every 5 minutes for
-- as long as DevHQ is open. These partial indexes let the manual records be read in
-- proportion to their own number, however many automated results exist. Nothing is rebuilt
-- and no row changes.
CREATE INDEX observations_manual ON observations(observed_at) WHERE source = 'manual';
CREATE INDEX events_manual ON events(occurred_at) WHERE source = 'manual';
