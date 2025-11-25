# Kubernetes Deployment

The `deployment/` folder contains a self-contained Helmfile setup that can launch both
Bitcoin Core and the `brc721` indexer/API on the same Kubernetes namespace. This folder
is intentionally decoupled from the rest of the repo so you can copy it into an ops repo
or wire it into your favourite GitOps tool.

## Prerequisites

- Helm 3.12+
- [helmfile](https://github.com/helmfile/helmfile)
- Access to a Kubernetes cluster (k3d/kind/minikube for `local`, DigitalOcean/other for prod)
- A container registry that hosts the `brc721` image you want to run (defaults to `ghcr.io/asini/brc721`)

## Layout

```
deployment/
├── charts/
│   ├── bitcoind        # StatefulSet + Services for Bitcoin Core
│   └── brc721          # StatefulSet + Services for this repo
├── helmfile.yaml.gotmpl
├── values-local.yaml
└── values-digitalocean.yaml
```

- `helmfile.yaml.gotmpl` wires the two charts together. Set `tag=<image-tag>` at runtime
  to pin the `brc721` image you pushed (defaults to Helm chart appVersion when unset).
- `values-*.yaml` hold per-environment overrides. `namespace`, `storageClassName`,
  `chain`, and `tag` live at the root, while chart-specific knobs are under `bitcoind`
  and `brc721`.

## Configuring credentials

1. **Bitcoin Core RPC auth**
   - Generate a hardened `rpcauth` entry at https://jlopp.github.io/bitcoin-core-rpc-auth-generator/
     using the username/password you want the `brc721` app to use.
   - Paste the resulting line into `bitcoind.rpc.rpcauth`.
   - Also set `bitcoind.rpc.user`, `bitcoind.rpc.password`, and mirror those values under
     `brc721.rpc.user` / `.password` so the app signs requests with matching creds.
   - For local/dev, it is acceptable to leave the defaults (`dev/dev`) and skip `rpcauth`.

2. **REST API auth (optional)**
   - If you want callers to supply an API token, create a Kubernetes secret that contains
     an `api-token` key, then set `brc721.apiToken.existingSecret` (and `.key` if you used
     a different key name). Leaving it blank disables auth.

3. **Persistent volumes**
   - `bitcoind.storage.size` controls the chainstate volume (default 100Gi locally, 1.5TiB
     on DigitalOcean). Update as needed.
   - `brc721.persistence.size` reserves capacity for the SQLite ledger. You can set
     `persistence.enabled=false` to use an `emptyDir` for throwaway environments.

## Commands

Render or apply the manifests by picking an environment defined in `helmfile.yaml.gotmpl`.

```bash
# Dry-run the local stack (uses values-local.yaml and regtest settings)
helmfile -e local template

# Deploy to a local cluster (k3d/minikube/kind) and watch the pods
helmfile -e local apply && kubectl -n brc721-local-dev get pods

# Deploy to production on DigitalOcean; tag determines the brc721 image
helmfile --set tag="v0.1.0" --environment prod-digitalocean apply
```

### Useful values to override

| Key | Description |
| --- | --- |
| `tag` | Shortcut for selecting the `brc721` container tag without editing values files. |
| `bitcoind.rpc.extraArgs` | Add raw `bitcoind` flags (e.g. pruning, custom peers). |
| `bitcoind.service.type` | Switch between `ClusterIP`, `NodePort`, or `LoadBalancer`. |
| `brc721.service.type` | Control whether the API is internal (`ClusterIP`) or public (`LoadBalancer`). |
| `brc721.rpc.existingSecret` | Reference a secret that holds `rpcuser`/`rpcpassword` if you do not want them in plain text values. |
| `brc721.persistence.enabled` | Toggle persistence for the SQLite database. |

## Tips

- When targeting DigitalOcean, make sure the cluster has the `do-block-storage` storage
  class (or update `storageClassName` / the chart overrides to match your CSI driver).
- Both charts create headless services (`*-headless`) so the StatefulSets keep stable
  identities. Reach the RPC interface through the regular `bitcoind` service.
- `helmDefaults.createNamespace=false` because many users prefer pre-provisioning namespaces.
  Run `kubectl create ns <namespace>` beforehand or switch the flag to `true`.
