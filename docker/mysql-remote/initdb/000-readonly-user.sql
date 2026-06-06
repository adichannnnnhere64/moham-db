-- SELECT-only remote user. Proves the sync engine works against a read-only
-- remote (it only ever issues SHOW/SELECT on the remote, never writes there).
-- Runs before schema/seed (000- prefix); granting on the db.* is fine before
-- the tables exist.
CREATE USER IF NOT EXISTS 'readonly'@'%' IDENTIFIED BY 'readonly';
GRANT SELECT ON sync_remote_test.* TO 'readonly'@'%';
FLUSH PRIVILEGES;
