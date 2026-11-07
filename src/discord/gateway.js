/**
 * Cliente mínimo do Gateway do Discord para conta de usuário.
 *
 * Responsável por: HELLO/heartbeat/ACK, IDENTIFY, RESUME, RECONNECT (op 7),
 * INVALID_SESSION (op 9), reconexão com backoff e close codes fatais.
 *
 * Eventos emitidos:
 *   'dispatch' (t, d)   - todo evento op 0 (emitido de forma síncrona: caminho quente)
 *   'ready'    (d)      - READY (nova sessão)
 *   'resumed'  ()       - sessão retomada
 *   'fatal'    (code, reason) - erro não recuperável (ex.: token inválido)
 */

const { EventEmitter } = require('node:events');
const WebSocket = require('ws');
const { PROPERTIES, CAPABILITIES, USER_AGENT, API_VERSION } = require('./properties');

const DEFAULT_URL = 'wss://gateway.discord.gg';
const QUERY = `/?v=${API_VERSION}&encoding=json`;

const OP = {
  DISPATCH: 0,
  HEARTBEAT: 1,
  IDENTIFY: 2,
  RESUME: 6,
  RECONNECT: 7,
  INVALID_SESSION: 9,
  HELLO: 10,
  HEARTBEAT_ACK: 11,
  GUILD_SUBSCRIPTIONS_BULK: 37,
};

const FATAL_CLOSE_CODES = {
  4004: 'Token inválido (Authentication failed)',
  4010: 'Shard inválido',
  4011: 'Sharding necessário',
  4012: 'Versão de API inválida',
  4013: 'Intents inválidos',
  4014: 'Intents não permitidos',
};

// Close codes após os quais a sessão não pode ser retomada.
const NON_RESUMABLE_CODES = new Set([4007, 4009]);

// Fechar com 1000/1001 invalida a sessão; usamos 4000 para poder dar RESUME.
const RECONNECT_CLOSE_CODE = 4000;

class Gateway extends EventEmitter {
  constructor(token) {
    super();
    this.token = token;
    this.ws = null;
    this.seq = null;
    this.sessionId = null;
    this.resumeUrl = null;
    this.user = null;
    this.heartbeatTimer = null;
    this.heartbeatAck = true;
    this.attempts = 0;
    this.nextDelay = null;
    this.reconnectTimer = null;
    this.destroyed = false;
  }

  connect() {
    if (this.destroyed) return;
    const base = this.sessionId && this.resumeUrl ? this.resumeUrl : DEFAULT_URL;
    const ws = new WebSocket(base + QUERY, {
      headers: { 'User-Agent': USER_AGENT, Origin: 'https://discord.com' },
      perMessageDeflate: false,
    });
    this.ws = ws;

    ws.on('message', (data) => this._onMessage(data));
    ws.on('close', (code, reason) => this._onClose(ws, code, reason ? reason.toString() : ''));
    ws.on('error', (err) => console.error(`❌ Erro no gateway: ${err.message}`));
  }

  send(op, d) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ op, d }));
      return true;
    }
    return false;
  }

  /**
   * Pede ao Discord para enviar eventos de tópicos/mensagens desse canal.
   * Necessário em servidores grandes, onde contas de usuário não recebem
   * todos os eventos por padrão.
   */
  subscribeChannel(guildId, channelId) {
    if (!guildId || !channelId) return false;
    return this.send(OP.GUILD_SUBSCRIPTIONS_BULK, {
      subscriptions: {
        [guildId]: {
          typing: true,
          threads: true,
          activities: true,
          member_updates: false,
          channels: { [channelId]: [[0, 99]] },
        },
      },
    });
  }

  destroy() {
    this.destroyed = true;
    this._stopHeartbeat();
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    if (this.ws) {
      try { this.ws.close(1000); } catch { /* ignore */ }
    }
  }

  // ───────────────────────────────────────────────────────────────

  _onMessage(data) {
    let packet;
    try {
      packet = JSON.parse(data.toString());
    } catch {
      return;
    }

    switch (packet.op) {
      case OP.DISPATCH:
        if (packet.s != null) this.seq = packet.s;
        this._onDispatch(packet.t, packet.d);
        break;
      case OP.HELLO:
        this._startHeartbeat(packet.d.heartbeat_interval);
        if (this.sessionId && this.seq != null) this._resume();
        else this._identify();
        break;
      case OP.HEARTBEAT_ACK:
        this.heartbeatAck = true;
        break;
      case OP.HEARTBEAT:
        this._sendHeartbeat();
        break;
      case OP.RECONNECT:
        console.log('🔄 Gateway pediu reconexão...');
        this._reconnectNow();
        break;
      case OP.INVALID_SESSION:
        if (!packet.d) this._clearSession();
        console.log(`⚠️ Sessão inválida (${packet.d ? 'retomável' : 'nova sessão'}). Reconectando...`);
        this.nextDelay = 1000 + Math.floor(Math.random() * 4000);
        this._reconnectNow();
        break;
      default:
        break;
    }
  }

  _onDispatch(t, d) {
    if (t === 'READY') {
      this.sessionId = d.session_id;
      this.resumeUrl = d.resume_gateway_url || null;
      this.user = d.user;
      this.attempts = 0;
    } else if (t === 'RESUMED') {
      this.attempts = 0;
    }

    // Caminho quente primeiro: listeners de dispatch.
    this.emit('dispatch', t, d);

    if (t === 'READY') this.emit('ready', d);
    else if (t === 'RESUMED') this.emit('resumed');
  }

  _identify() {
    this.send(OP.IDENTIFY, {
      token: this.token,
      capabilities: CAPABILITIES,
      properties: PROPERTIES,
      presence: { status: 'unknown', since: 0, activities: [], afk: false },
      compress: false,
      client_state: { guild_versions: {} },
    });
  }

  _resume() {
    this.send(OP.RESUME, { token: this.token, session_id: this.sessionId, seq: this.seq });
  }

  _startHeartbeat(interval) {
    this._stopHeartbeat();
    this.heartbeatAck = true;
    const beat = () => {
      if (!this.heartbeatAck) {
        console.log('⚠️ Heartbeat sem resposta. Reconectando...');
        if (this.ws) this.ws.terminate();
        return;
      }
      this.heartbeatAck = false;
      this._sendHeartbeat();
    };
    // Primeiro heartbeat com jitter, depois em intervalo fixo.
    this.heartbeatTimer = setTimeout(() => {
      beat();
      this.heartbeatTimer = setInterval(beat, interval);
    }, Math.floor(interval * Math.random()));
  }

  _sendHeartbeat() {
    this.send(OP.HEARTBEAT, this.seq);
  }

  _stopHeartbeat() {
    if (this.heartbeatTimer) {
      clearTimeout(this.heartbeatTimer);
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  _clearSession() {
    this.sessionId = null;
    this.resumeUrl = null;
    this.seq = null;
  }

  _reconnectNow() {
    if (this.ws) {
      try { this.ws.close(RECONNECT_CLOSE_CODE); } catch { this.ws.terminate(); }
    }
  }

  _onClose(ws, code, reason) {
    if (ws !== this.ws) return; // socket antigo
    this._stopHeartbeat();
    this.ws = null;
    if (this.destroyed) return;

    if (FATAL_CLOSE_CODES[code]) {
      this.emit('fatal', code, FATAL_CLOSE_CODES[code]);
      return;
    }

    if (NON_RESUMABLE_CODES.has(code)) this._clearSession();

    let delay = this.nextDelay;
    this.nextDelay = null;
    if (delay == null) {
      delay = this.attempts === 0 ? 0 : Math.min(1000 * 2 ** (this.attempts - 1), 30000);
      delay += Math.floor(Math.random() * 500);
    }
    this.attempts++;

    console.log(`⚠️ Desconectado do gateway (code ${code}${reason ? `: ${reason}` : ''}). Reconectando em ${delay}ms...`);
    this.reconnectTimer = setTimeout(() => this.connect(), delay);
  }
}

module.exports = { Gateway, OP };
