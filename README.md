# brc721

A Rust daemon that connects to a Bitcoin Core node, scans blocks for BRC‑721 data, persists state to SQLite and exposes a small HTTP API. It also provides a CLI for managing a watch‑only wallet and building protocol transactions.

---

## Storage

- Default data directory: `./.brc721/`
- Per‑network layout: `./.brc721/<network>/brc721.sqlite` (e.g. `./.brc721/regtest/brc721.sqlite`)
- Use `--reset` to wipe the SQLite database before starting.

---

## Environment & configuration

Configuration is based on a `.env` file in the project root (loaded automatically) plus CLI flags. 

Typical flow:
- Create a `.env` file in the project root with your RPC settings.
- Optionally override the file used by setting `DOTENV_PATH` (e.g. `.env.local`).

### Daemon / scanner

Global flags (apply to both daemon and subcommands):

- `--reset`
  - Base directory for persistent data (default: `.brc721/`). The effective path is `DIR/<network>/brc721.sqlite`.

### Logging & HTTP

- `--log-file PATH` / `BRC721_LOG_FILE`
  - Optional log file (in addition to stderr).

Example `.env`:

```bash
BITCOIN_RPC_URL=http://127.0.0.1:8332
BITCOIN_RPC_USER=dev
BITCOIN_RPC_PASS=dev
```

---

## Running the daemon

The daemon runs the block scanner and HTTP API together. If no subcommand is given, the app runs as a long‑lived process.

```bash
# Using the default .env
cargo run

# Using a custom dotenv file
DOTENV_PATH=.env.local cargo run

# Override some runtime parameters
cargo run -- \
  --data-dir .brc721/ \
  --confirmations 3 \
  --batch-size 100 \
  --api-listen 127.0.0.1:8083
```

---

## CLI

See `cargo run -- --help` for the current set of wallet and transaction subcommands and their flags.

---

## HTTP API


Endpoints (default base URL: `http://127.0.0.1:8083`):

- `GET /health`

  ```json
  { "status": "ok", "uptime_secs": 123 }
  ```

- `GET /state`

  ```json
  { "last": { "height": 123, "hash": "..." } }
  ```

- `GET /collections`

  ```json
  {
    "collections": [
      {
        "id": "<collection-id>",
        "evmCollectionAddress": "0x...",
        "rebaseable": false
      }
    ]
  }
  ```

Example curl:

```bash
curl http://127.0.0.1:8083/health
curl http://127.0.0.1:8083/state
curl http://127.0.0.1:8083/collections
```

---

## Local regtest with Docker

A minimal regtest setup is provided via `docker-compose.yml:13-32` (service `bitcoind-testnet`).

1. Start the node:

   ```bash
   docker compose up -d
   ```

2. Configure the app by `.env.testing` used for local regtest:

   ```bash
   DOTENV_PATH=.env.testing cargo run
   ```

3. Initialize the wallet:

   ```bash
   cargo run -- wallet init --mnemonic "word1 ... word12"
   ```
4. Get a receive address and mine funds to it:

   ```bash
   ADDR=$(cargo run -- wallet address 2>&1 | grep -Eo 'bcrt1[0-9a-z]+' | tail -n1)
   echo "$ADDR"

   docker exec bitcoind-testnet bitcoin-cli \
     -regtest -rpcuser=dev -rpcpassword=dev \
     generatetoaddress 101 "$ADDR"
   ```

5. Inspect balances via Core and via the app:

   ```bash
   # Core view
   docker exec bitcoind-testnet bitcoin-cli \
     -regtest -rpcuser=dev -rpcpassword=dev -getinfo

   # App view
   cargo run -- wallet balance
   ```


