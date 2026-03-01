# Cloudflare Worker Setup Guide

## Prerequisites
- Cloudflare account
- Wrangler CLI installed (`npm install -g wrangler`)
- Discord bot token
- Node.js v16+

## Installation Steps

### 1. Install Dependencies
```bash
npm install
```

### 2. Login to Cloudflare
```bash
wrangler login
```

### 3. Create KV Namespaces
```bash
wrangler kv:namespace create "TOKENS_KV"
wrangler kv:namespace create "ENCRYPTION_KV"
wrangler kv:namespace create "LOGS_KV"
```

Copy the namespace IDs and update `wrangler.toml`.

### 4. Set Shared Secret
Update the `SHARED_SECRET` in `wrangler.toml` or use:
```bash
wrangler secret put SHARED_SECRET
```

### 5. Set Encryption Key
Generate a random 32-byte base64 key:
```powershell
# PowerShell
$bytes = New-Object byte[] 32
[Security.Cryptography.RNGCryptoServiceProvider]::Create().GetBytes($bytes)
[Convert]::ToBase64String($bytes)
```

Then set it:
```bash
wrangler kv:key put --namespace-id=YOUR_ENCRYPTION_KV_ID "encryption_key" "YOUR-BASE64-KEY"
```

### 6. Set Discord Bot Token
```bash
wrangler kv:key put --namespace-id=YOUR_TOKENS_KV_ID "tokens" '{"active":"YOUR-BOT-TOKEN"}'
```

### 7. Deploy Worker
```bash
wrangler deploy
```

## Configuration

### wrangler.toml Structure
```toml
name = "kurinium-proxy"
main = "src/index.js"
compatibility_date = "2025-12-22"

[[kv_namespaces]]
binding = "TOKENS_KV"
id = "YOUR_TOKENS_KV_ID"

[[kv_namespaces]]
binding = "ENCRYPTION_KV"
id = "YOUR_ENCRYPTION_KV_ID"

[[kv_namespaces]]
binding = "LOGS_KV"
id = "YOUR_LOGS_KV_ID"

[vars]
SHARED_SECRET = "YOUR-SHARED-SECRET-KEY"
```

## Testing

1. Get your worker URL from Cloudflare dashboard
2. Test the debug endpoint:
```bash
curl https://YOUR-WORKER.workers.dev/debug
```

3. Update your Rust client's `src/config.rs` with the worker URL

## Endpoints

- `POST /key` - Retrieve encryption key (requires shared secret)
- `POST /api/*` - Discord API proxy (encrypted)
- `GET /debug` - Debug information

## Troubleshooting

### Error: "Encryption Key Not Found"
- Ensure you've set the encryption key in KV storage
- Check the namespace ID matches in wrangler.toml

### Error: "No active token"
- Set the Discord bot token in TOKENS_KV
- Format: `{"active":"BOT-TOKEN-HERE"}`

### Error: "Unauthorized Key Request"
- Verify shared secret matches between client and worker
- Check for whitespace or encoding issues
