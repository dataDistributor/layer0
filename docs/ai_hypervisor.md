# AI Hypervisor

The AI hypervisor gathers chain context and queries OpenAI to provide operator-friendly insights.

## Config
Set in `config/dxid.toml` or env overrides:
```
[ai]
openai_api_key = "sk-..."
model = "gpt-4o-mini"
```
Env override example: `DXID__AI__OPENAI_API_KEY`.

## Flow
1. Collects summary (height, peers, prompt).
2. Builds a concise system/user prompt.
3. Calls OpenAI Chat Completions API via `reqwest`.
4. Returns the answer to REST (`/ai/query`), gRPC (`AiQuery`), CLI (`dxid ai`), or TUI (AI tab).

## Extending
- Add richer summaries from storage (recent blocks, identity stats).
- Integrate `dxid-vectors` KNN lookups to surface similar states/anomalies.
- Apply rate limiting and caching before production exposure.
