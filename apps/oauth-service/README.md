# AFS Auth Broker

Minimal OAuth broker for AgentFS connector auth.

The broker exists for providers whose OAuth REST API requires a confidential
client secret. The local `afs` client keeps the normal desktop UX: start a
localhost callback, open the provider consent page, receive the authorization
code, and store returned credentials in the OS credential store. This service
only performs the confidential token exchange and refresh calls.

## Flow

```text
afs CLI -> broker /start
afs CLI <- authorization_url, state, signed session
afs CLI -> browser -> Notion OAuth consent
Notion -> localhost callback on the user's machine
afs CLI -> broker /exchange with code, state, session, redirect_uri
broker -> Notion token endpoint with client_secret
broker -> afs CLI with access token and refresh handle
```

Refresh is similarly narrow:

```text
afs CLI -> broker /refresh with refresh_token_handle
broker -> Notion token endpoint with client_secret
broker -> afs CLI with new access token and new refresh handle
```

The broker does not persist page content or tokens. In `handle` mode, it returns
an encrypted opaque refresh handle instead of the raw provider refresh token.

## API

### `POST /v1/oauth/notion/start`

Request:

```json
{
  "redirect_uri": "http://localhost:8757/oauth/notion/callback"
}
```

Response:

```json
{
  "connector": "notion",
  "client_id": "public-client-id",
  "authorization_url": "https://api.notion.com/v1/oauth/authorize?...",
  "redirect_uri": "http://localhost:8757/oauth/notion/callback",
  "session": "signed-session",
  "state": "opaque-state",
  "expires_in": 600
}
```

### `POST /v1/oauth/notion/exchange`

Request:

```json
{
  "session": "signed-session",
  "state": "opaque-state",
  "code": "provider-authorization-code",
  "redirect_uri": "http://localhost:8757/oauth/notion/callback"
}
```

Response includes the provider access token and either `refresh_token_handle` or
`refresh_token`, depending on `AFS_TOKEN_MODE`.

### `POST /v1/oauth/notion/refresh`

Request:

```json
{
  "refresh_token_handle": "afsrh_v1..."
}
```

## Local Development

```sh
npm install
cp .dev.vars.example .dev.vars
npm run dev
```

Run checks:

```sh
npm run check
```

## Required Secrets

- `AFS_BROKER_SESSION_SECRET`: signs short-lived OAuth sessions.
- `AFS_REFRESH_HANDLE_KEY`: encrypts opaque refresh handles in `handle` mode.
- `AFS_NOTION_CLIENT_ID`: Notion OAuth client ID.
- `AFS_NOTION_CLIENT_SECRET`: Notion OAuth client secret.

## Deployment

Recommended first deployment target: Cloudflare Workers.

This service is stateless, TypeScript-native, latency-insensitive, and only
needs provider secrets plus outbound HTTPS. Workers fit that shape well. Use
`wrangler secret put` for secrets, keep only non-sensitive defaults in
`wrangler.toml`, and deploy from GitHub Actions once the repository is pushed.

Alternatives:

- Vercel Functions: good if the rest of the web stack already lives on Vercel.
- Fly.io: good if we later need a long-running service, regional control, or a
  stateful companion process.

Cloudflare Workers is the smallest operational surface for this broker.
