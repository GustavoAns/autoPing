/**
 * Cliente REST do Discord via undici Pool (conexões TLS persistentes).
 * `sendMessage` é o caminho quente: nada de I/O extra antes do POST.
 */

const { Pool } = require('undici');
const { SUPER_PROPERTIES, USER_AGENT, LOCALE, API_VERSION } = require('./properties');

const DISCORD_EPOCH = 1420070400000n;
const API = `/api/v${API_VERSION}`;

function nonce() {
  return ((BigInt(Date.now()) - DISCORD_EPOCH) << 22n).toString();
}

function createRest(token) {
  const pool = new Pool('https://discord.com', {
    connections: 10,
    pipelining: 1,
    keepAliveTimeout: 300000,
    keepAliveMaxTimeout: 600000,
    connect: { rejectUnauthorized: true },
  });

  const HEADERS = Object.freeze({
    authorization: token,
    'content-type': 'application/json',
    'user-agent': USER_AGENT,
    'x-super-properties': SUPER_PROPERTIES,
    'x-discord-locale': LOCALE,
    origin: 'https://discord.com',
  });

  let keepWarmTimer = null;

  /**
   * Request genérico. Sempre consome o body (obrigatório no undici para
   * devolver a conexão ao pool).
   */
  async function request(method, path, body) {
    const res = await pool.request({
      path: API + path,
      method,
      headers: HEADERS,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.body.text();
    let data = null;
    if (text) {
      try { data = JSON.parse(text); } catch { data = text; }
    }
    return { status: res.statusCode, data };
  }

  function httpError(status, data) {
    const err = new Error(`HTTP ${status}${data && data.message ? ` - ${data.message}` : ''}`);
    err.status = status;
    err.code = data && data.code;
    return err;
  }

  /**
   * Envia mensagem num canal/tópico. Em 429 aguarda retry_after e tenta 1x.
   */
  async function sendMessage(channelId, content, retried = false) {
    const res = await pool.request({
      path: `${API}/channels/${channelId}/messages`,
      method: 'POST',
      headers: HEADERS,
      body: JSON.stringify({ content, nonce: nonce(), tts: false, flags: 0 }),
    });

    if (res.statusCode < 400) {
      await res.body.dump();
      return;
    }

    let data = null;
    try { data = await res.body.json(); } catch { /* body vazio */ }

    if (res.statusCode === 429 && !retried) {
      const waitMs = Math.ceil(((data && data.retry_after) || 1) * 1000);
      console.log(`   ⏳ Rate limit (429). Tentando novamente em ${waitMs}ms...`);
      await new Promise((r) => setTimeout(r, waitMs));
      return sendMessage(channelId, content, true);
    }

    throw httpError(res.statusCode, data);
  }

  async function getChannel(channelId) {
    const { status, data } = await request('GET', `/channels/${channelId}`);
    if (status >= 400) throw httpError(status, data);
    return data;
  }

  async function deleteMessage(channelId, messageId) {
    const { status, data } = await request('DELETE', `/channels/${channelId}/messages/${messageId}`);
    if (status >= 400) throw httpError(status, data);
  }

  async function ping() {
    const res = await pool.request({ path: `${API}/gateway`, method: 'GET', headers: { 'user-agent': USER_AGENT } });
    await res.body.dump();
  }

  /**
   * Abre a conexão TLS e mantém "quente" com ping a cada 30s.
   */
  async function warmup() {
    if (keepWarmTimer) return;
    try {
      await ping();
      console.log('🔥 Conexão com Discord API pré-aquecida!');
    } catch {
      console.log('⚠️ Warmup falhou, mantendo keep-warm mesmo assim');
    }
    keepWarmTimer = setInterval(() => ping().catch(() => {}), 30000);
    keepWarmTimer.unref();
    console.log('🔥 Keep-warm ativado (ping a cada 30s)');
  }

  async function close() {
    if (keepWarmTimer) clearInterval(keepWarmTimer);
    keepWarmTimer = null;
    await pool.close().catch(() => {});
  }

  return { request, sendMessage, getChannel, deleteMessage, warmup, close };
}

module.exports = { createRest };
