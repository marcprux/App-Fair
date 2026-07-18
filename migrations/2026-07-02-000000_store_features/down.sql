DROP INDEX idx_apps_pkg;
DROP TABLE installs;
ALTER TABLE apps DROP COLUMN signer;
ALTER TABLE repos DROP COLUMN mirrors;
ALTER TABLE repos DROP COLUMN priority;
ALTER TABLE repos DROP COLUMN fingerprint;
