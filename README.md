brc721

A simple Rust app that connects to a Bitcoin Core node and streams blocks.

Storage

- SQLite (rusqlite) at ./.brc721/brc721.sqlite, created automatically.
- No CSV or legacy fallback.
- Use --reset to delete the database file before starting.

Environment

- BITCOIN_RPC_URL, BITCOIN_RPC_USER/PASS or BITCOIN_RPC_COOKIE
- BRC721_DB_PATH (optional, default ./.brc721/brc721.sqlite)

CLI

- --confirmations N: process up to tip - N (default 3)
- --batch-size SIZE: batch processing size (default 100)
- --reset: delete DB before start

Local testing with Bitcoin Core (regtest)

Option A: Native install

- Requirements: Bitcoin Core v26+ installed and on PATH
- Start a fresh regtest node with RPC enabled:

```
bitcoind -regtest -datadir=.bitcoin-regtest -fallbackfee=0.0001 -server=1 -txindex=1 -rpcallowip=127.0.0.1 -rpcuser=dev -rpcpassword=dev -rpcport=18443 -port=18444 -daemon
```

- Configure the app to point at regtest:

Create a .env (or edit existing):

```
BITCOIN_RPC_URL=http://127.0.0.1:18443
BITCOIN_RPC_USER=dev
BITCOIN_RPC_PASS=dev
```

- Generate some blocks so there is data:

```
bitcoin-cli -regtest -rpcuser=dev -rpcpassword=dev -rpcport=18443 -generate 101
```

- Run the app:

```
cargo run
```

Option B: Docker (recommended for quick setup)

- Requirements: Docker + Docker Compose
- Start regtest node in Docker:

```
docker compose up -d
```

This uses bitcoin/bitcoin with configs under docker/bitcoin/*. RPC is mapped to 127.0.0.1:18443.

- Configure the app:

```
cp .env.example .env
BITCOIN_RPC_URL=http://127.0.0.1:18443
BITCOIN_RPC_USER=dev
BITCOIN_RPC_PASS=dev
```

- Create wallet and generate blocks inside the container:

```
docker compose exec bitcoind bitcoin-cli -regtest -rpcuser=dev -rpcpassword=dev createwallet default
docker compose exec bitcoind bitcoin-cli -regtest -rpcuser=dev -rpcpassword=dev -generate 101
```

- Run the app/tests against Docker node:

```
cargo run
# Tests will read .env.test automatically
cargo test -- --nocapture
```

- Stop and clean up:

```
docker compose down
```

Integration test helper

- You can also run a simple smoke test that connects to the regtest node and asserts it can read the tip and a block header. Make sure the node is running and has blocks.

```
cargo test -- --nocapture
```

Troubleshooting

- If you see connection refused, ensure bitcoind is running on regtest and RPC creds/port match .env
- If auth fails, try using cookie auth instead of user/pass. Typically: BITCOIN_RPC_COOKIE=./.bitcoin-regtest/regtest/.cookie
