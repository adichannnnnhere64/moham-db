-- Empty local database (NO tables) used to test a fresh bootstrap: sync_all +
-- create_missing must build the full schema and copy all rows into it.
CREATE DATABASE IF NOT EXISTS sync_local_fresh;
GRANT ALL PRIVILEGES ON sync_local_fresh.* TO 'sync'@'%';
FLUSH PRIVILEGES;
