#!/bin/bash

# Run this script INSIDE your 'rarm-db' directory.

# 1. Initialize the workspace (top-level Cargo.toml)
cat > Cargo.toml <<EOF
[workspace]
members = [
    "crates/storage",
    "crates/sql",
    "crates/network",
    "crates/server",
    "crates/client"
]
resolver = "2"
EOF

# 2. Create the 'crates' directory
mkdir -p crates

# 3. Create the 'storage' library (Disk, Pages, BufferPool)
cargo new --lib crates/storage --name rarmdb_storage --vcs none

# 4. Create the 'sql' library (AST, Parser)
cargo new --lib crates/sql --name rarmdb_sql --vcs none

# 5. Create the 'network' library (Packets, Protocol)
cargo new --lib crates/network --name rarmdb_network --vcs none

# 6. Create the 'server' binary
cargo new --bin crates/server --name rarmdb_server --vcs none

# 7. Create the 'client' binary
cargo new --bin crates/client --name rarmdb_client --vcs none

# 8. Link dependencies (Server depends on everything else)
# We append these to the server's Cargo.toml
cat >> crates/server/Cargo.toml <<EOF

[dependencies]
rarmdb_storage = { path = "../storage" }
rarmdb_sql = { path = "../sql" }
rarmdb_network = { path = "../network" }
tokio = { version = "1", features = ["full"] } # Async runtime
EOF

echo "Rust workspace 'rarm-db' setup successfully!"
echo "To run tests: cargo test"
echo "To build: cargo build"