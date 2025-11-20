# brc721

A Rust daemon that connects to a Bitcoin Core node, scans blocks for BRC‑721 data, persists state to SQLite and exposes a small HTTP API. It also provides a CLI for managing a watch‑only wallet and building protocol transactions.

---

## Storage

- Default data directory: `./.brc721/`
- Per‑network layout: `./.brc721/<network>/brc721.sqlite` (e.g. `./.brc721/regtest/brc721.sqlite`)
- Use `--reset` to wipe the SQLite database before starting.

---

## Environment & configuration

Configuration is a mix of CLI flags and environment variables. The CLI parser lives in `src/cli/args.rs:6-105` and the runtime context in `src/context.rs:7-42`.

### Core RPC

- `BITCOIN_RPC_URL` (required unless passed via `--rpc-url`)
  - Default: `http://127.0.0.1:8332`
- `BITCOIN_RPC_USER` / `BITCOIN_RPC_PASS`
  - Default: `dev` / `dev` (also usable via `--rpc-user` / `--rpc-pass`)

### Daemon / scanner

Global flags (apply to both daemon and subcommands):

- `--start HEIGHT` / `BRC721_START_BLOCK`
  - Initial block height if no prior state exists (default: `923580`).
- `--confirmations N`
  - Only process up to `tip - N` (default: `3`).
- `--batch-size SIZE`
  - Batch size for block processing (default: `1`).
- `--reset`
  - Delete the SQLite database before starting.
- `--data-dir DIR`
  - Base directory for persistent data (default: `.brc721/`). The effective path is `DIR/<network>/brc721.sqlite`.

### Logging & HTTP

- `--log-file PATH` / `BRC721_LOG_FILE`
  - Optional log file (in addition to stderr).
- `--api-listen ADDR` / `BRC721_API_LISTEN`
  - REST API bind address (default: `127.0.0.1:8083`).

### Env loading

- `DOTENV_PATH` (optional)
  - Path to a dotenv file loaded before parsing CLI args (default: `.env`).
  - See `src/cli/args.rs:107-112`.

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

If a subcommand is provided (`wallet` or `tx`), the app runs that command once and exits (see `src/app.rs:184-193`).

---

## CLI usage

Top‑level help:

```bash
cargo run -- --help
```

### Wallet commands

Subcommands are defined in `src/cli/wallet_cmd.rs:3-44` and wired in `src/commands/wallet.rs:10-23`.

- Initialize (or reuse) wallet and Core watch‑only wallet:

  ```bash
  cargo run -- wallet init \
    --mnemonic "word1 word2 ... word12" \
    [--passphrase "optional-passphrase"]
  ```

  - If a wallet already exists in `data_dir`, it is loaded and the Core watch‑only wallet is set up again if needed.

- Generate a new 12‑word BIP39 mnemonic (no side effects):

  ```bash
  cargo run -- wallet generate
  ```

- Get the next receive address (stateful, persisted):

  ```bash
  cargo run -- wallet address
  ```

- Show wallet balances (as reported by the Core watch‑only wallet and local index):

  ```bash
  cargo run -- wallet balance
  ```

- Trigger a Core wallet rescan:

  ```bash
  cargo run -- wallet rescan
  ```

### Transaction commands

Subcommands are defined in `src/cli/tx_cmd.rs:4-68` and implemented in `src/commands/tx.rs:37-83`.

- Register a BRC‑721 collection:

  ```bash
  cargo run -- tx register-collection \
    --evm-collection-address 0x0123456789abcdef0123456789abcdef01234567 \
    [--rebaseable] \
    [--fee-rate 1.0] \
    [--passphrase "optional-passphrase"]
  ```

- Send a specific amount (in satoshis) to an address on the connected network:

  ```bash
  cargo run -- tx send-amount \
    bcrt1...target-address... \
    --amount-sat 100000 \
    [--fee-rate 1.0] \
    [--passphrase "optional-passphrase"]
  ```

Both transaction commands load the same wallet data as the wallet subcommands and will fail if no wallet has been initialized.

---

## HTTP API

The HTTP server is implemented in `src/rest.rs:47-72`. It currently exposes read‑only endpoints; wallet operations are driven through the CLI, not HTTP.

Start the daemon (scanner + API):

```bash
cargo run
# or explicitly
cargo run -- --api-listen 127.0.0.1:8083
```

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

There is currently no HTTP authentication layer; protect the bind address at the infrastructure level if needed.

---

## Local regtest with Docker

A minimal regtest setup is provided via `docker-compose.yml:13-32` (service `bitcoind-testnet`).

1. Start the node:

   ```bash
   docker compose up -d
   ```

2. Configure the app (example `.env.local` used for regtest):

   ```bash
   BITCOIN_RPC_URL=http://127.0.0.1:18443
   BITCOIN_RPC_USER=dev
   BITCOIN_RPC_PASS=dev
   ```

3. Initialize the wallet:

   ```bash
   cargo run -- wallet init --mnemonic "word1 ... word12"
   ```

   Example log (wallet creation + Core watch‑only wallet): see `src/commands/wallet.rs:25-63`.

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

6. Run tests (many tests require Docker / regtest):

   ```bash
   cargo test -- --nocapture
   ```

7. Stop the node:

   ```bash
   docker compose down
   ```

---

## Troubleshooting

- Connection refused:
  - Ensure `bitcoind` is running and `BITCOIN_RPC_URL` / credentials match.
- Wrong network / address errors:
  - Addresses passed to `tx send-amount` must match the detected network (`src/context.rs:46-52`).
- State looks stale:
  - Check `--confirmations`, `--batch-size` and logs; use `--reset` to rescan from `--start`.
