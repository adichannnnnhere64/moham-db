.PHONY: test test-unit test-integration test-up test-wait test-down test-clean

export SYNC_REMOTE_HOST := 127.0.0.1
export SYNC_REMOTE_PORT := 3308
export SYNC_REMOTE_DB   := sync_remote_test
export SYNC_REMOTE_USER := sync
export SYNC_REMOTE_PASS := sync
export SYNC_LOCAL_HOST  := 127.0.0.1
export SYNC_LOCAL_PORT  := 3307
export SYNC_LOCAL_DB    := sync_local_test
export SYNC_LOCAL_USER  := sync
export SYNC_LOCAL_PASS  := sync
export SYNC_REMOTE_RO_USER := readonly
export SYNC_REMOTE_RO_PASS := readonly
export SYNC_LOCAL_FRESH_DB := sync_local_fresh

# Full test run: start containers (initdb seeds on first run), run all tests.
# Use `make test-clean && make test` for a guaranteed fresh-seed run.
test: test-up test-wait test-unit test-integration

test-unit:
	cargo test --manifest-path src-tauri/Cargo.toml --no-default-features -- --test-threads=1

test-integration:
	cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features integration -- --test-threads=1

test-up:
	docker compose up -d mysql-local mysql-remote

test-wait:
	@echo "Waiting for mysql-local..."
	@until docker compose exec -T mysql-local mysqladmin ping -h127.0.0.1 -usync -psync --silent 2>/dev/null; do sleep 1; done
	@echo "Waiting for mysql-remote..."
	@until docker compose exec -T mysql-remote mysqladmin ping -h127.0.0.1 -usync -psync --silent 2>/dev/null; do sleep 1; done
	@echo "Both ready."

test-down:
	docker compose down

# Remove volumes so next test-up re-runs docker-entrypoint-initdb.d scripts.
test-clean:
	docker compose down -v --remove-orphans
