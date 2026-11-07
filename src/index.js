/**
 * Discord AutoPing
 * Monitora um canal e responde automaticamente quando um novo tópico é criado.
 * Quem responde primeiro fica com a vaga: latência é a métrica principal.
 *
 * ⚠️ AVISO: O uso de self-bots viola os Termos de Serviço do Discord.
 * Use por sua conta e risco.
 *
 * Comandos: `!autoPing ajuda` (enviado pela própria conta, em qualquer chat).
 */

const { Config, loadToken } = require('./config');
const { createRest } = require('./discord/rest');
const { Gateway } = require('./discord/gateway');
const { Cache } = require('./discord/cache');
const { HumanizedWaiter } = require('./core/humanized');
const { createResponder } = require('./core/responder');
const { createCommands, createChannelValidator, modeText } = require('./commands');

const token = loadToken();
const config = new Config();
const rest = createRest(token);
const gateway = new Gateway(token);
const cache = new Cache();

let cachedChannelInfo = null;

// ═══════════════════════════════════════════════════════════════════
// STATUS
// ═══════════════════════════════════════════════════════════════════

const validateChannel = createChannelValidator({ rest, cache });

async function printStatus() {
  if (!cachedChannelInfo && config.channelId) {
    const v = await validateChannel(config.channelId);
    if (v.valid) cachedChannelInfo = v.info;
  }
  const user = gateway.user;
  console.log('═'.repeat(50));
  console.log(`👤 Logado como: ${user ? user.username : '?'}`);
  console.log(`📺 Monitorando: ${cachedChannelInfo || config.channelId || 'Não configurado'}`);
  console.log(`💬 Mensagem: "${config.autoMessage || 'Não configurada'}"`);
  console.log(`🕐 Modo: ${modeText(config)}`);
  console.log(`💤 Sleep após responder: ${config.sleepAfterReply ? 'Ligado' : 'Desligado'}`);
  console.log(`⚡ Status: ${config.enabled ? '🟢 Ativo' : '🔴 Desativado'}`);
  console.log('═'.repeat(50));
  console.log('📝 Comandos disponíveis: !autoPing ajuda');
  console.log('⏳ Aguardando criação de novos tópicos...\n');
}

// ═══════════════════════════════════════════════════════════════════
// RESPONDER + MODO HUMANIZADO
// ═══════════════════════════════════════════════════════════════════

const responder = createResponder({
  config,
  rest,
  onSent: () => { printStatus().catch(() => {}); },
});

const humanized = new HumanizedWaiter({
  getWaitMs: () => config.waitForMessage,
  getDelayMs: () => config.humanizedDelay,
  send: (threadId, startTime, reason) => responder.send(threadId, startTime, reason),
});
responder.setHumanized(humanized);

humanized.on('owner-message', ({ threadId, early }) => {
  console.log(`   📨 1ª mensagem do criador detectada${early ? ' (antes do tópico)' : ''} em ${threadId}`);
});
humanized.on('timeout', ({ threadId }) => {
  console.log(`   ⏰ Timeout em ${threadId} - enviando mesmo assim`);
});
humanized.on('cancelled', ({ count }) => {
  console.log(`   🚫 ${count} tópico(s) pendente(s) cancelado(s)`);
});

// ═══════════════════════════════════════════════════════════════════
// REAÇÕES A MUDANÇAS DE CONFIG
// ═══════════════════════════════════════════════════════════════════

async function subscribeMonitoredChannel(guildIdHint) {
  if (!config.channelId) return;
  const guildId = guildIdHint || (cache.getChannel(config.channelId) || {}).guildId;
  if (guildId && gateway.subscribeChannel(guildId, config.channelId)) {
    console.log('📡 Inscrito nos eventos do canal monitorado');
  }
}

config.on('change', (key, value) => {
  if (key === 'enabled' && !value) humanized.cancelAll();
  if (key === 'channelId') {
    humanized.cancelAll();
    cachedChannelInfo = null;
    subscribeMonitoredChannel();
  }
});

// ═══════════════════════════════════════════════════════════════════
// GATEWAY
// ═══════════════════════════════════════════════════════════════════

const commands = createCommands({
  config,
  rest,
  cache,
  validateChannel,
  getSelfId: () => gateway.user && gateway.user.id,
  printStatus,
});

gateway.on('dispatch', (t, d) => {
  // Caminho quente primeiro.
  responder.handleDispatch(t, d);

  cache.handleDispatch(t, d);
  if (t === 'MESSAGE_CREATE') commands.handleMessage(d);
});

gateway.on('ready', async (d) => {
  cache.loadReady(d);
  console.log('═'.repeat(50));
  console.log('🚀 Discord AutoPing Iniciado!');

  rest.warmup();

  if (config.channelId) {
    const v = await validateChannel(config.channelId);
    if (v.valid) {
      cachedChannelInfo = v.info;
      subscribeMonitoredChannel(v.guildId);
    } else {
      console.log(`⚠️ Canal configurado mas com problema: ${v.error}`);
      console.log('   Use !autoPing canal ID para configurar um canal válido');
    }
  }

  await printStatus();
});

gateway.on('resumed', () => console.log('🔄 Sessão retomada'));

gateway.on('fatal', (code, reason) => {
  console.error(`❌ Erro fatal no gateway (${code}): ${reason}`);
  if (code === 4004) console.error('   💡 O token está inválido. Gere um novo token.');
  shutdown(1);
});

// ═══════════════════════════════════════════════════════════════════
// PROCESSO
// ═══════════════════════════════════════════════════════════════════

let shuttingDown = false;
async function shutdown(code = 0) {
  if (shuttingDown) return;
  shuttingDown = true;
  console.log('\n👋 Encerrando AutoPing...');
  humanized.destroy();
  gateway.destroy();
  await rest.close();
  process.exit(code);
}

process.on('SIGINT', () => shutdown(0));
process.on('SIGTERM', () => shutdown(0));
process.on('unhandledRejection', (error) => {
  console.error('❌ Erro não tratado:', error && error.message ? error.message : error);
});

console.log('🔄 Conectando ao Discord...\n');
gateway.connect();
