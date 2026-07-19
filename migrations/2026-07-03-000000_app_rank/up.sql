-- An optional catalog-defined ordering. The App Fair Index V2 extension adds a top-level `rank`
-- array (application ids, highest-ranked first); each app's position becomes its `rank`. Apps a
-- catalog didn't rank — and every app in a plain F-Droid catalog without the extension — get the
-- BIGINT-max sentinel, so `ORDER BY rank ASC` keeps ranked apps first and sinks the rest below them
-- (no NULLS-LAST needed). The default browse order (#15) becomes rank, then recency.
ALTER TABLE apps ADD COLUMN rank BIGINT NOT NULL DEFAULT 9223372036854775807;
CREATE INDEX apps_rank ON apps(rank);
