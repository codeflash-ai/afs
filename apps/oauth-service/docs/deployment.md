# Deployment

## Recommended: Cloudflare Workers

The broker is a small stateless HTTP service. Cloudflare Workers is the best
initial fit because it runs TypeScript directly, supports environment secrets,
and does not require operating a server.

Production setup:

```sh
npm install
wrangler secret put AFS_BROKER_SESSION_SECRET
wrangler secret put AFS_REFRESH_HANDLE_KEY
wrangler secret put AFS_NOTION_CLIENT_ID
wrangler secret put AFS_NOTION_CLIENT_SECRET
wrangler deploy
```

Configure the Notion OAuth integration with the exact localhost callback used by
AFS:

```text
http://localhost:8757/oauth/notion/callback
http://127.0.0.1:8757/oauth/notion/callback
```

Use a stable production URL such as:

```text
https://auth.agentfs.dev
```

The AFS client should have:

```text
AFS_AUTH_BROKER_URL=https://auth.agentfs.dev
AFS_NOTION_OAUTH_CLIENT_ID=<public client id>
```

The client ID may also be fetched from `/v1/oauth/notion/start`; keeping it in
the AFS binary is fine because it is not confidential.

## GitHub Actions

Use Cloudflare's deploy action with a Cloudflare API token stored as a GitHub
secret. Provider OAuth secrets should stay in Cloudflare Workers secrets, not in
GitHub Actions secrets, unless the deployment workflow explicitly manages them.

## Alternatives

Vercel Functions are also viable and have a simple GitHub integration. Choose
Vercel if AFS already has a Vercel web property and we want one hosting surface.

Fly.io is a better fit if this grows into a long-running relay, needs regional
process control, or starts using local stateful services.
