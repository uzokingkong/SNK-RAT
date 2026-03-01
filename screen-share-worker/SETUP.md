# Screen Share Worker Setup Guide

## Prerequisites
- Cloudflare account with Workers and Durable Objects enabled
- Wrangler CLI installed
- Discord Activity created (optional, for Discord integration)

## Installation Steps

### 1. Install Dependencies
```bash
npm install
```

### 2. Create KV Namespace
```bash
wrangler kv:namespace create "ACTIVE_SESSIONS"
```

Update `wrangler.toml` with the namespace ID.

### 3. Deploy Worker
```bash
wrangler deploy
```

The Durable Objects will be automatically created on first deployment.

## Configuration

### wrangler.toml Structure
```toml
name = "screen-share"
main = "src/index.js"
compatibility_date = "2024-01-01"

[[kv_namespaces]]
binding = "ACTIVE_SESSIONS"
id = "YOUR_KV_ID"

[durable_objects]
bindings = [
  { name = "ROOM", class_name = "ScreenShareRoom" }
]

[[migrations]]
tag = "v1"
new_classes = ["ScreenShareRoom"]
```

### Discord Activity Setup (Optional)

1. Create an application at [Discord Developer Portal](https://discord.com/developers/applications)
2. Navigate to "Activities" tab
3. Set the URL Mappings:
   - Root Mapping: `https://YOUR-WORKER.workers.dev`
4. Update the client ID in `src/index.js`:
   ```javascript
   const discordSdk = new window.DiscordSDK.DiscordSDK('YOUR-CLIENT-ID');
   ```

## Usage

### For Rust Client (Publisher)

The Rust client automatically connects to the worker and publishes the screen stream.

Connection URL format:
```
wss://YOUR-WORKER.workers.dev?role=publisher&room=CHANNEL_ID
```

### For Viewers

#### Option 1: Discord Activity
- Launch the activity from Discord
- Automatically joins the channel's room

#### Option 2: Direct URL
```
https://YOUR-WORKER.workers.dev?room=CHANNEL_ID
```

#### Option 3: Active Sessions List
- Visit `https://YOUR-WORKER.workers.dev`
- Select from active sessions

## Features

### Real-time Screen Streaming
- AV1 codec for efficient compression
- WebCodecs API for hardware-accelerated decoding
- Automatic keyframe caching for late joiners

### Remote Control
- Mouse movement and clicks
- Keyboard input
- Fullscreen mode with keyboard lock
- Works seamlessly through the browser

### Session Management
- Each room is identified by Discord channel ID
- Active sessions stored in KV with 24h TTL
- Automatic cleanup on disconnect

## Browser Compatibility

### Required Features
- WebCodecs API (Chrome 94+, Edge 94+)
- WebSocket support
- Canvas API

### Recommended Browsers
- Google Chrome (latest)
- Microsoft Edge (latest)
- Brave (latest)

Safari and Firefox do not currently support WebCodecs.

## Troubleshooting

### "Browser not supported" Error
- Ensure you're using Chrome, Edge, or Brave
- Update to the latest browser version

### No Video Display
- Check browser console for decoder errors
- Verify WebCodecs support: `'VideoDecoder' in window`
- Publisher must be connected and sending frames

### Remote Control Not Working
- Ensure you're in fullscreen mode
- Check WebSocket connection status
- Verify publisher is receiving control messages

### High Latency
- Check network connection
- Reduce stream quality in Rust client
- Use a closer Cloudflare region

## API Endpoints

- `GET /` - Viewer HTML interface
- `WS /?role=publisher&room=ID` - Publisher WebSocket
- `WS /?role=viewer&room=ID` - Viewer WebSocket
- `GET /api/sessions` - List active sessions
- `POST /.proxy/api/token` - Discord OAuth token exchange

## Performance Tips

1. **Optimize Stream Quality**: Adjust encoder settings in Rust client
2. **Minimize Latency**: Use Cloudflare's global network
3. **Resource Usage**: Durable Objects are billed per request
4. **Scale**: Each room runs in its own Durable Object

## Security Considerations

- No authentication by default (add if needed)
- All traffic encrypted via TLS
- Consider adding room password protection
- Monitor active sessions for abuse

## Monitoring

View logs in Cloudflare dashboard:
```bash
wrangler tail
```

Check active sessions:
```bash
curl https://YOUR-WORKER.workers.dev/api/sessions
```
