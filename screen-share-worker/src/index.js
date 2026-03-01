/**
 * Cloudflare Worker 진입점
 */
export default {
  async fetch(request, env, ctx) {
    try {
      const url = new URL(request.url);
      const corsHeaders = {
        'Access-Control-Allow-Origin': '*',
        'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
        'Access-Control-Allow-Headers': 'Content-Type',
        'Content-Security-Policy': "frame-ancestors https://*.discord.com https://discord.com;",
        'X-Frame-Options': 'ALLOW-FROM https://discord.com',
      };

      if (request.method === 'OPTIONS') {
        return new Response(null, { headers: corsHeaders });
      }

      // Handle OAuth token exchange for Discord Activity
      if (url.pathname === '/.proxy/api/token' && request.method === 'POST') {
        return handleTokenExchange(request, corsHeaders);
      }

      // API to list active sessions
      if (url.pathname === '/api/sessions' && request.method === 'GET') {
        const list = await env.ACTIVE_SESSIONS.list();
        return new Response(JSON.stringify(list), {
          headers: { 'Content-Type': 'application/json', ...corsHeaders }
        });
      }

      const upgradeHeader = request.headers.get('Upgrade');
      if (!upgradeHeader || upgradeHeader.toLowerCase() !== 'websocket') {
        return handleHttpRequest(url, env, corsHeaders);
      }

      const roomName = url.searchParams.get('room') || 'main';
      const id = env.ROOM.idFromName(roomName);
      const roomObject = env.ROOM.get(id);

      return roomObject.fetch(request);
    } catch (err) {
      return new Response(`Error: ${err.message}`, { status: 500 });
    }
  }
};

async function handleTokenExchange(request, corsHeaders) {
  try {
    const { code } = await request.json();

    // Exchange code for access token with Discord
    const response = await fetch('https://discord.com/api/oauth2/token', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/x-www-form-urlencoded',
      },
      body: new URLSearchParams({
        client_id: env.DISCORD_CLIENT_ID,
        client_secret: env.DISCORD_CLIENT_SECRET,
        grant_type: 'authorization_code',
        code: code,
      }),
    });

    const tokens = await response.json();

    return new Response(JSON.stringify(tokens), {
      headers: { 'Content-Type': 'application/json', ...corsHeaders }
    });
  } catch (error) {
    return new Response(JSON.stringify({ error: error.message }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders }
    });
  }
}

function handleHttpRequest(url, env, corsHeaders) {
  return new Response(VIEWER_HTML, {
    headers: { 'Content-Type': 'text/html;charset=UTF-8', ...corsHeaders }
  });
}

export class ScreenShareRoom {
  constructor(state, env) {
    this.state = state;
    this.env = env;
    this.sessions = new Map();
    this.latestFrame = null;
  }

  async fetch(request) {
    const url = new URL(request.url);
    const role = url.searchParams.get('role') || 'viewer';
    const room = url.searchParams.get('room');

    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];

    server.accept();
    this.sessions.set(server, { role, connectedAt: Date.now() });

    if (role === 'publisher' && room) {
      this.env.ACTIVE_SESSIONS.put(room, JSON.stringify({
        startedAt: Date.now(),
        id: room
      }), { expirationTtl: 86400 }).catch(console.error);
    }

    server.addEventListener('message', (event) => {
      if (role === 'publisher') {
        if (event.data instanceof ArrayBuffer) {
          this.latestFrame = event.data;
          this.broadcast(event.data, server);
        }
      } else {
        this.forwardToPublisher(event.data);
      }
    });

    server.addEventListener('close', () => {
      this.sessions.delete(server);
      if (role === 'publisher' && room) {
        this.env.ACTIVE_SESSIONS.delete(room).catch(console.error);
      }
    });
    server.addEventListener('error', () => {
      this.sessions.delete(server);
      if (role === 'publisher' && room) {
        this.env.ACTIVE_SESSIONS.delete(room).catch(console.error);
      }
    });

    if (role === 'viewer' && this.latestFrame) {
      server.send(this.latestFrame);
    }

    return new Response(null, { status: 101, webSocket: client });
  }

  broadcast(data, sender) {
    for (const [ws, info] of this.sessions.entries()) {
      if (info.role === 'viewer' && ws !== sender) {
        // [Optimization] 버퍼가 2MB 이상 쌓인 클라이언트는 프레임 드랍 (렉 방지)
        if (ws.bufferedAmount > 2 * 1024 * 1024) {
          continue;
        }
        try { ws.send(data); } catch { this.sessions.delete(ws); }
      }
    }
  }

  forwardToPublisher(data) {
    for (const [ws, info] of this.sessions.entries()) {
      if (info.role === 'publisher') {
        try { ws.send(data); } catch { this.sessions.delete(ws); }
      }
    }
  }
}

const VIEWER_HTML = `<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
  <title>원격 제어 Pro</title>
  <script src="https://unpkg.com/@discord/embedded-app-sdk"></script>
  <style>
    :root {
        --primary: #5865f2;
        --bg-darker: #0f0f0f;
        --bg-light: #2a2a2a;
        --border: #3a3a3a;
    }
    
    * { margin: 0; padding: 0; box-sizing: border-box; }
    
    body { 
        background: var(--bg-darker); overflow: hidden; font-family: sans-serif;
        width: 100%; height: 100vh; position: fixed; touch-action: none; color: white;
    }
    
    #app-container { display: flex; flex-direction: column; height: 100vh; }
    #screen-container { flex: 1; display: flex; justify-content: center; align-items: center; background: #000; position: relative; overflow: hidden; }
    canvas { display: none; box-shadow: 0 0 20px #000; max-width: 100%; max-height: 100%; }
    
    /* Toolbar */
    #toolbar { background: #1a1a1a; padding: 10px; display: flex; gap: 8px; z-index: 300; align-items: center; border-bottom: 1px solid var(--border); }
    .tool-btn { background: var(--bg-light); border: 1px solid var(--border); color: white; padding: 8px 15px; border-radius: 6px; font-size: 14px; cursor: pointer; }
    .tool-btn:active { background: var(--primary); }
    .mobile-only { display: none; }
    
    /* Hidden Input for Keyboard */
    /* opacity 0 but pointer-events auto to ensure it catches focus */
    .input-wrapper { position: relative; display: inline-block; }
    #hidden-input { 
        position: absolute; top: 0; left: 0; width: 100%; height: 100%; 
        opacity: 0; cursor: text; z-index: 10;
        pointer-events: auto !important; 
    }
    
    /* Stats & Notifs */
    #fps-display { margin-left: auto; font-family: monospace; }
    #notifications { position: fixed; top: 60px; right: 20px; z-index: 600; display: flex; flex-direction: column; gap: 10px; pointer-events: none; }
    .notification { background: #333; border-left: 4px solid var(--primary); padding: 10px 15px; border-radius: 4px; pointer-events: auto; animation: slideIn 0.3s; }
    @keyframes slideIn { from { transform: translateX(100%); opacity: 0; } to { transform: translateX(0); opacity: 1; } }

    /* Side Panel */
    #side-panel { position: fixed; right: -320px; top: 0; bottom: 0; width: 320px; background: #1a1a1a; transition: right 0.3s; z-index: 400; padding: 20px; box-shadow: -5px 0 15px rgba(0,0,0,0.5); }
    #side-panel.open { right: 0; }
    
    /* FAB */
    #fab-menu { display: none; position: fixed; bottom: 30px; right: 30px; flex-direction: column-reverse; gap: 10px; z-index: 500; }
    .fab { width: 56px; height: 56px; background: var(--primary); border-radius: 50%; display: flex; align-items: center; justify-content: center; font-size: 24px; box-shadow: 0 4px 10px rgba(0,0,0,0.5); }
    .fab-mini { width: 48px; height: 48px; font-size: 20px; display: none; background: #333; }
    .fab-mini.show { display: flex; }
    
    /* Mobile Controls */
    #mobile-controls { display: none; position: fixed; bottom: 0; left: 0; right: 0; background: rgba(0,0,0,0.95); padding: 10px; z-index: 450; flex-direction: column; gap: 5px; }
    #mobile-controls.open { display: flex; }
    .mobile-row { display: flex; gap: 5px; }
    .key-btn { flex: 1; padding: 12px; background: #333; border-radius: 5px; text-align: center; color: white; border: 1px solid #444; font-weight: bold; }
    .key-btn:active { background: var(--primary); }

    /* Overlay */
    #overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.9); display: flex; flex-direction: column; justify-content: center; align-items: center; z-index: 1000; color: white; }
  </style>
</head>
<body>
  <div id="app-container">
    <div id="toolbar">
      <button class="tool-btn" id="btn-fullscreen">⛶ 전체화면</button>
      
      <!-- Wrapper ensures click hits the input -->
      <div class="input-wrapper mobile-only">
          <button class="tool-btn">⌨️ 입력</button>
          <input type="text" id="hidden-input" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck="false">
      </div>
      
      <button class="tool-btn mobile-only" id="btn-vkeys">🎮 키패드</button>
      <div id="fps-display">0 FPS</div>
    </div>
    
    <div id="screen-container">
      <canvas id="screen-canvas"></canvas>
      <div id="overlay">
        <div style="margin-bottom:15px; font-size:24px;">📡</div>
        <div id="status-text">연결 대기 중...</div>
      </div>
    </div>
    
    <div id="side-panel">
        <h3 style="margin-bottom:15px;">📋 클립보드</h3>
        <p style="color:#aaa; font-size:13px;">텍스트를 입력하고 전송하세요.</p>
        <input type="text" id="clipboard-input" style="width:100%; padding:10px; margin:10px 0; background:#333; border:1px solid #555; color:white; border-radius:4px;" placeholder="내용 입력...">
        <button class="tool-btn" id="send-clipboard" style="width:100%">PC로 전송</button>
        <button class="tool-btn" id="close-panel" style="width:100%; margin-top:10px; background:#444;">닫기</button>
    </div>
    
    <div id="mobile-controls">
        <div class="mobile-row"><div class="key-btn" data-key="Escape">ESC</div><div class="key-btn" data-key="Tab">TAB</div><div class="key-btn" data-key="Control">CTRL</div><div class="key-btn" data-key="Alt">ALT</div></div>
        <div class="mobile-row"><div class="key-btn" data-key="ArrowUp">↑</div><div class="key-btn" data-key="ArrowDown">↓</div><div class="key-btn" data-key="ArrowLeft">←</div><div class="key-btn" data-key="ArrowRight">→</div></div>
        <div class="mobile-row"><div class="key-btn" data-key="Backspace">BKSP</div><div class="key-btn" data-key="Enter">ENTER</div><div class="key-btn" data-key="Meta">WIN</div><div class="key-btn" id="hide-vkeys">▼</div></div>
    </div>
    
    <div id="fab-menu" class="mobile-only">
        <div class="fab fab-mini" id="fab-clip">📋</div>
        <div class="fab main" id="fab-main">+</div>
    </div>
    
    <div id="notifications"></div>
  </div>

  <script>
    const canvas = document.getElementById('screen-canvas');
    const ctx = canvas.getContext('2d');
    const hiddenInput = document.getElementById('hidden-input');
    const isTouch = 'ontouchstart' in window || navigator.maxTouchPoints > 0;
    
    let ws, lastUrl;
    let stats = { fps: 0, frames: 0, lastCheck: Date.now() };

    // --- Mode Setup ---
    if (isTouch) {
        document.querySelectorAll('.mobile-only').forEach(el => el.style.display = 'inline-block');
        document.getElementById('fab-menu').style.display = 'flex';
        showNotify('모바일 모드: 터치로 제어');
    } else {
        document.querySelectorAll('.mobile-only').forEach(el => el.style.display = 'none');
        
        // PC Keyboard Logic
        const handlePCKey = (e, type) => {
            if(e.target.tagName==='INPUT') return;
            if(['F5','F11','F12'].includes(e.key)) return;
            e.preventDefault();
            sendControl({ type, key: e.key });
        };
        window.addEventListener('keydown', e => handlePCKey(e, 'key_down'));
        window.addEventListener('keyup', e => handlePCKey(e, 'key_up'));
        showNotify('PC 모드: 키보드/마우스 제어');
    }

    function showNotify(msg) {
        const d = document.createElement('div');
        d.className = 'notification';
        d.innerText = msg;
        document.getElementById('notifications').appendChild(d);
        setTimeout(() => d.remove(), 2500);
    }
    
    function sendControl(data) {
        if(ws && ws.readyState === 1) ws.send(JSON.stringify(data));
    }

    function connect(room) {
        const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
        ws = new WebSocket(proto + '//' + location.host + '?role=viewer&room=' + room);
        ws.binaryType = 'arraybuffer';
        
        ws.onopen = () => { document.getElementById('status-text').innerText = "연결됨"; showNotify('서버 접속 성공'); };
        ws.onmessage = (e) => {
            if (e.data instanceof ArrayBuffer) {
                const arr = new Uint8Array(e.data);
                const isKey = arr[0] === 1;
                const blob = new Blob([e.data.slice(isKey?1:0)], {type:'image/jpeg'});
                const img = new Image();
                img.onload = () => {
                    document.getElementById('overlay').style.display = 'none';
                    canvas.style.display = 'block';
                    if(canvas.width !== img.width) { canvas.width=img.width; canvas.height=img.height; fit(); }
                    ctx.drawImage(img, 0, 0);
                    stats.frames++;
                    if(lastUrl) URL.revokeObjectURL(lastUrl);
                    lastUrl = null;
                };
                lastUrl = URL.createObjectURL(blob);
                img.src = lastUrl;
            }
        };
        ws.onclose = () => setTimeout(() => connect(room), 3000);
    }

    // --- Core Layout ---
    function fit() {
        if(!canvas.width) return;
        const ctr = document.getElementById('screen-container');
        const aspect = canvas.width / canvas.height;
        let w = ctr.clientWidth, h = w / aspect;
        if(h > ctr.clientHeight) { h = ctr.clientHeight; w = h * aspect; }
        canvas.style.width = w + 'px'; canvas.style.height = h + 'px';
    }
    window.addEventListener('resize', fit);
    setInterval(() => {
        if(Date.now() - stats.lastCheck >= 1000) {
            document.getElementById('fps-display').innerText = stats.frames + ' FPS';
            stats.frames = 0; stats.lastCheck = Date.now();
        }
    }, 1000);

    // --- Input Logic ---
    if(!isTouch) {
        const getP = (e) => { const r = canvas.getBoundingClientRect(); return { x: (e.clientX - r.left)/r.width, y: (e.clientY - r.top)/r.height }; };
        canvas.addEventListener('mousedown', e => { e.preventDefault(); sendControl({type:'mouse_down', button: e.button}); });
        canvas.addEventListener('mouseup', e => { e.preventDefault(); sendControl({type:'mouse_up', button: e.button}); });
        canvas.addEventListener('mousemove', e => { const p=getP(e); sendControl({type:'mouse_move', x:p.x, y:p.y}); });
        canvas.addEventListener('contextmenu', e => e.preventDefault());
        canvas.addEventListener('wheel', e => { e.preventDefault(); sendControl({type:'mouse_scroll', dy: Math.sign(e.deltaY)*-100}); });
    }

    if(isTouch) {
        let touchMode='none', tStart=null, tTimer=null;
        const getT = (t) => { const r = canvas.getBoundingClientRect(); return { x: (t.clientX - r.left)/r.width, y: (t.clientY - r.top)/r.height }; };
        
        // Improved Drag Logic
        canvas.addEventListener('touchstart', e => {
            e.preventDefault();
            if(e.touches.length===1) {
                touchMode='move';
                const t = e.touches[0];
                tStart = {x: t.clientX, y: t.clientY};
                const p = getT(t);
                sendControl({type:'mouse_move', x:p.x, y:p.y});
                
                // Long Press for Drag
                tTimer = setTimeout(() => {
                    touchMode='drag';
                    if(navigator.vibrate) navigator.vibrate([30,50,30]); // Distinct vibrate
                    sendControl({type:'mouse_down', button:0});
                    showNotify('🖱️ 드래그 모드 시작');
                }, 400); // Slightly faster
            } else if(e.touches.length===2) {
                clearTimeout(tTimer); touchMode='scroll'; tStart={y:e.touches[0].clientY};
            }
        }, {passive:false});

        canvas.addEventListener('touchmove', e => {
            e.preventDefault();
            if(touchMode==='move') {
                const t = e.touches[0];
                // Increased threshold for jitter
                const d = Math.hypot(t.clientX-tStart.x, t.clientY-tStart.y);
                if(d > 15) clearTimeout(tTimer); // Jitter tolerance 15px
                
                const p = getT(t);
                sendControl({type:'mouse_move', x:p.x, y:p.y});
            } else if(touchMode==='drag') {
                const p = getT(e.touches[0]);
                sendControl({type:'mouse_move', x:p.x, y:p.y});
            } else if(touchMode==='scroll') {
               const y = e.touches[0].clientY;
               if(Math.abs(y - tStart.y) > 5) {
                   sendControl({type:'mouse_scroll', dy: (y-tStart.y)*2});
                   tStart.y = y;
               }
            }
        }, {passive:false});

        canvas.addEventListener('touchend', e => {
            e.preventDefault(); clearTimeout(tTimer);
            if(touchMode==='drag') {
                sendControl({type:'mouse_up', button:0});
                showNotify('드래그 종료');
            } else if(touchMode==='move') {
                sendControl({type:'mouse_down', button:0});
                setTimeout(()=>sendControl({type:'mouse_up', button:0}), 40);
            }
            touchMode='none';
        }, {passive:false});
        
        // Virtual Keys
        document.querySelectorAll('.key-btn').forEach(b => {
             b.addEventListener('touchstart', e => { e.preventDefault(); b.style.background='var(--primary)'; sendControl({type:'key_down', key: b.dataset.key}); if(navigator.vibrate)navigator.vibrate(20); });
             b.addEventListener('touchend', e => { e.preventDefault(); b.style.background='#333'; sendControl({type:'key_up', key: b.dataset.key}); });
        });
        document.getElementById('btn-vkeys').onclick = () => document.getElementById('mobile-controls').classList.toggle('open');
        document.getElementById('hide-vkeys').onclick = () => document.getElementById('mobile-controls').classList.remove('open');
        
        // Keyboard: rely on direct input events
        hiddenInput.addEventListener('input', e => {
            if(e.data) for(let c of e.data) { sendControl({type:'key_down', key:c}); setTimeout(()=>sendControl({type:'key_up', key:c}),30); }
            if(e.inputType==='deleteContentBackward') { sendControl({type:'key_down', key:'Backspace'}); sendControl({type:'key_up', key:'Backspace'}); }
            hiddenInput.value='';
        });
        hiddenInput.addEventListener('keydown', e => {
             if(e.key==='Enter') { e.preventDefault(); sendControl({type:'key_down', key:'Enter'}); sendControl({type:'key_up', key:'Enter'}); }
        });
        
        // FAB
        let fabOpen=false;
        document.getElementById('fab-main').onclick = () => { fabOpen = !fabOpen; document.getElementById('fab-main').innerText = fabOpen ? '×' : '+'; document.querySelectorAll('.fab-mini').forEach(e => e.classList.toggle('show', fabOpen)); };
        document.getElementById('fab-clip').onclick = () => document.getElementById('side-panel').classList.toggle('open');
    }

    document.getElementById('send-clipboard').onclick = () => { const t=document.getElementById('clipboard-input').value; if(t){sendControl({type:'clipboard_set', text:t}); document.getElementById('clipboard-input').value=''; showNotify('전송됨'); }};
    document.getElementById('close-panel').onclick = () => document.getElementById('side-panel').classList.remove('open');
    
    // Init
    const params = new URLSearchParams(location.search);
    if(params.get('room')) connect(params.get('room'));
    else fetch('/api/sessions').then(r=>r.json()).then(d=>{ if(d.keys.length) location.href='?room='+d.keys[0].name; else showNotify('공유 중인 세션 없음'); });
  </script>
</body>
</html>`;
