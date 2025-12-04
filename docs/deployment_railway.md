# Railway deployment

## Prerequisites
- Railway account and project.
- Railway Postgres add-on with pgvector enabled: `CREATE EXTENSION IF NOT EXISTS vector;`
- OpenAI API key for the AI hypervisor.

## Build & deploy
1. Clone the repo on your machine.
2. Build container locally (optional):
   ```bash
   docker build -t dxid-node .
   ```
3. On Railway, create a new service from the repo or from the Dockerfile.
4. Set environment variables:
   - `DXID_CONFIG=/app/config/dxid.toml`
   - `DATABASE_URL` (from Railway Postgres)
   - `DXID__DB__URL` (override config if desired)
   - `DXID__AI__OPENAI_API_KEY`
   - `RUST_LOG=info`
5. Mount or bake config: the Dockerfile copies `config/dxid.toml`; adjust values for Railway endpoints.
6. Expose ports:
   - REST: `8080`
   - gRPC: `50051`
   - P2P: `7000` (if public networking is allowed)

## Migrations
Storage crate auto-creates minimal tables on startup. For production, manage migrations separately.

## Running
The container entrypoint launches `dxid-node`. Logs should show REST/gRPC bind addresses and libp2p listen address.

## Health checks
- HTTP: `GET /health` on the REST port.
- gRPC: call `GetStatus` from the `dxid` service.
