export RUST_BACKTRACE := "short"

fmt:
    cargo fmt
    cd examples/basic && cargo fmt

test: test-crate test-example-basic

doc:
    RUSTDOCFLAGS="-Z unstable-options --document-hidden-items" cargo doc --all-features --document-private-items

test-no-pg: test-crate-no-pg test-example-basic

make-test-db:
    dropdb crdb-test || true
    createdb crdb-test
    sqlx migrate run --source src/server/migrations/ --database-url "postgres:///crdb-test?host=/run/postgresql"

rebuild-offline-queries: make-test-db
    cargo sqlx prepare --database-url "postgres:///crdb-test?host=/run/postgresql" -- --all-features --tests
    dropdb crdb-test

test-crate:
    SQLX_OFFLINE="true" cargo nextest run --all-features

test-crate-no-pg:
    SQLX_OFFLINE="true" cargo nextest run --all-features -E 'all() - test(server::postgres_db::)'

test-example-basic: build-example-basic-client test-example-basic-host

build-example-basic-client:
    cd examples/basic && CARGO_TARGET_DIR="target/wasm" RUSTFLAGS="-Zmacro-backtrace" cargo build --target wasm32-unknown-unknown -p client

test-example-basic-host:
    cd examples/basic && CARGO_TARGET_DIR="target/host" RUSTFLAGS="-Zmacro-backtrace" cargo test -p api -p server

fuzz-object-cache:
    cargo bolero test --all-features \
        -j 8 \
        cache::object_cache::tests::cache_state_stays_valid \
        --corpus-dir src/cache/object_cache/__fuzz__/cache__object_cache__tests__cache_state_stays_valid/corpus.nounit

fuzz-pg-basic:
    cargo bolero test --all-features \
        -j 8 \
        server::postgres_db::tests::fuzz_simple::db_keeps_invariants \
        --corpus-dir src/server/postgres_db/tests/__fuzz__/server__postgres_db__tests__db_keeps_invariants/corpus.nounit
