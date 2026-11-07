/**
 * Configuração do AutoPing.
 * Carrega o .env e expõe um objeto de configuração em memória que pode ser
 * alterado em runtime (comandos / futura interface). Toda alteração feita via
 * `config.set()` emite o evento 'change' (key, newValue, oldValue).
 */

require('dotenv').config({ quiet: true });
const { EventEmitter } = require('node:events');

// Tipos de canal que podem ter tópicos (threads):
// 0 = GUILD_TEXT, 5 = GUILD_ANNOUNCEMENT, 15 = GUILD_FORUM, 16 = GUILD_MEDIA
const THREAD_PARENT_TYPES = new Set([0, 5, 15, 16]);

const CHANNEL_TYPE_ICONS = { 0: '💬', 5: '📢', 15: '📋', 16: '🖼️' };

const DEFAULT_WAIT_MS = 5000;
const DEFAULT_HUMANIZED_DELAY_MS = 900;

function parseBool(value, fallback = false) {
  if (value === undefined || value === null || value === '') return fallback;
  return ['1', 'true', 'sim', 'yes', 'on'].includes(String(value).trim().toLowerCase());
}

function parseIntOr(value, fallback) {
  const n = parseInt(value, 10);
  return Number.isNaN(n) || n < 0 ? fallback : n;
}

class Config extends EventEmitter {
  constructor(env = process.env) {
    super();
    this.channelId = (env.CHANNEL_ID || '').trim();
    this.autoMessage = env.AUTO_MESSAGE || '';
    // Tempo máx. (ms) aguardando a 1ª msg do criador antes de enviar mesmo assim.
    this.waitForMessage = parseIntOr(env.WAIT_FOR_MESSAGE, DEFAULT_WAIT_MS) || DEFAULT_WAIT_MS;
    // Delay humanizado (ms) após a 1ª msg do criador. A ordem já é garantida
    // (nosso snowflake é sempre maior); o delay só deixa o tempo de resposta
    // mais "humano". 0 = mais rápido.
    this.humanizedDelay = parseIntOr(env.HUMANIZED_DELAY, DEFAULT_HUMANIZED_DELAY_MS);
    // Sleep: após responder um tópico, desliga (enabled=false) até `!autoPing on`.
    this.sleepAfterReply = parseBool(env.SLEEP_AFTER_REPLY, false);
    this.prefix = '!autoPing';
    this.enabled = true;
  }

  set(key, value) {
    const old = this[key];
    if (old === value) return;
    this[key] = value;
    this.emit('change', key, value, old);
  }

  snapshot() {
    return {
      channelId: this.channelId,
      autoMessage: this.autoMessage,
      waitForMessage: this.waitForMessage,
      humanizedDelay: this.humanizedDelay,
      sleepAfterReply: this.sleepAfterReply,
      enabled: this.enabled,
    };
  }
}

/**
 * Lê e valida o token. Encerra o processo se ausente.
 */
function loadToken(env = process.env) {
  const token = (env.DISCORD_TOKEN || '').trim();
  if (!token || token.includes('seu_token')) {
    console.error('❌ Erro: DISCORD_TOKEN não configurado no arquivo .env');
    process.exit(1);
  }
  return token;
}

function validateMessage(message) {
  if (!message || message.trim().length === 0) {
    return { valid: false, error: 'A mensagem não pode estar vazia.' };
  }
  if (message.length > 2000) {
    return { valid: false, error: 'A mensagem não pode ter mais de 2000 caracteres.' };
  }
  return { valid: true };
}

function isSnowflake(id) {
  return typeof id === 'string' && /^\d{17,20}$/.test(id);
}

module.exports = {
  Config,
  loadToken,
  validateMessage,
  isSnowflake,
  parseBool,
  THREAD_PARENT_TYPES,
  CHANNEL_TYPE_ICONS,
};
