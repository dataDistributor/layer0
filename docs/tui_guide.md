# TUI guide

Launch: `dxid` (no args) or `cargo run -p dxid-tui`.

## Tabs / keys
- `1` Dashboard
- `2` Wallet
- `3` Identities
- `4` Chains
- `5` Bridge
- `6` Mining
- `7` AI hypervisor chat
- `q` Quit

## Layout
- Top tab bar with section names.
- Content pane shows basic status or instructions. The AI tab has a prompt box and response area.

## AI tab
- Type your prompt; press Enter to send.
- The TUI will invoke the AI hypervisor (OpenAI-backed) to answer with chain context.

## Notes
- The TUI is intentionally minimal/fast; it can run connected to a local node via RPC or be extended for in-process calls.
