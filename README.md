# BRC721

`brc721` is a Rust app that implements the BRC-721 protocol described in the [BRC-721 research paper](https://eprint.iacr.org/2025/641).

It is experimental software with no warranty. See [LICENSE](LICENSE) for more details.

BRC-721 scales the creation, management, and trading of non-fungible tokens (NFTs) on Bitcoin by extending [Bridgeless Minting patterns](https://github.com/freeverseio/laos-whitepaper/blob/main/laos.pdf) previously used on other blockchains.

The protocol leverages on-chain Bitcoin data to handle all aspects of token ownership, including trading, while integrating a secondary consensus system for minting and optionally modifying token metadata. 

To minimize its on-chain footprint, the protocol utilizes the `OP_RETURN` mechanism for ownership records, while complementary NFT-related actions are stored on the LAOS blockchain. All data remains permanently on-chain, with no reliance on bridges or third-party operators.

The `OP_RETURN` pattern is heavily inspired by the [Runes Protocol](https://docs.ordinals.com/runes.html), which greatly simplifies previous patterns based on [Ordinal Theory](https://docs.ordinals.com/), used by [Inscriptions/Ordinals](https://ordinals.com/).

## The app

This is a simple Rust app that connects to a Bitcoin Core node and streams blocks, persists collections to SQLite, and now exposes a minimal HTTP API to drive wallet operations and create collections.

Storage

- SQLite (rusqlite) at ./.brc721/brc721.sqlite, created automatically (you just need to create the .brc721 folder first manually)
- No CSV or legacy fallback.
- Use --reset to delete the database file before starting.

Environment

- BITCOIN_RPC_URL, BITCOIN_RPC_USER/PASS or BITCOIN_RPC_COOKIE
- BRC721_DB_PATH (optional, default ./.brc721/brc721.sqlite)
- BRC721_API_BIND (optional, default 127.0.0.1:8080)
- BRC721_API_TOKEN (optional auth token; if set, requests must send x-api-key: <token> or Authorization: Bearer <token>)

CLI

- --confirmations N: process up to tip - N (default 3)
- --batch-size SIZE: batch processing size (default 100)
- --reset: delete DB before start
- Subcommands:
  - wallet-init --name <wallet>
  - wallet-newaddress --name <wallet>
  - wallet-balance --name <wallet>
  - collection-create --name <wallet> --laos-hex <20-byte-hex> [--rebaseable] [--fee-rate <sat/vB>]
  - serve [--bind <addr:port>]  Start the HTTP API (defaults to BRC721_API_BIND or 127.0.0.1:8080)

HTTP API

Start the server (scanner runs concurrently, persisting to SQLite):

```
# start API and the background scanner together
cargo run -- serve --bind 127.0.0.1:8080
# pass debug and confirmations flags to control scanner verbosity/lag
cargo run -- -d -c 3 serve --bind 127.0.0.1:8080
# or via env
BRC721_API_BIND=0.0.0.0:8080 cargo run -- serve
# optional auth
export BRC721_API_TOKEN=secret123
```

Endpoints:

- POST /wallet/init

  Request body:
  {"name":"default"}

  Response:
  {"ok":true,"name":"default"}

- GET /wallet/{name}/address

  Response:
  {"address":"bcrt1..."}

- GET /wallet/{name}/balance

  Response:
  {"balance_btc": 12.345}

- POST /collections

  Request body:
  {"wallet":"default","laos_hex":"<40-hex>","rebaseable":false,"fee_rate":1.0}

  Response:
  {"txid":"<txid>"}

If BRC721_API_TOKEN is set, include either header:
- x-api-key: <token>
- Authorization: Bearer <token>

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

- Run the scanner or API:

```
# scanner
cargo run
# API server
cargo run -- serve --bind 127.0.0.1:8080
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
# scanner
cargo run
# api
cargo run -- serve
# tests
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
