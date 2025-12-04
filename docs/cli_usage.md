# CLI usage (`dxid`)

Running `dxid` with no arguments launches the TUI by default. Use `--help-mode` to print help instead.

## Init
```
dxid init --config config/dxid.toml
```

## Node
```
dxid node start --config config/dxid.toml
dxid node status
```

## Wallet
```
dxid wallet new --name main --password "secret"
dxid wallet list
```

## AI hypervisor
```
dxid ai "How healthy is the network?"
```

## Notes
- `DXID_CONFIG` env var overrides the config path for node startup.
- Wallets are stored under `~/.dxid/wallets` by default.
