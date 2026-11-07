/**
 * Utilitário para listar todos os canais (que suportam tópicos) dos servidores.
 * Facilita a obtenção dos IDs para configuração.
 *
 * Uso: npm run list-channels
 */

const { loadToken, THREAD_PARENT_TYPES, CHANNEL_TYPE_ICONS } = require('../config');
const { Gateway } = require('../discord/gateway');
const { Cache } = require('../discord/cache');

// Servidores grandes podem chegar via GUILD_CREATE depois do READY.
const SETTLE_MS = 3000;

const token = loadToken();
const gateway = new Gateway(token);
const cache = new Cache();
let settleTimer = null;

function printAndExit() {
  const user = gateway.user;
  console.log('═'.repeat(70));
  console.log('📋 LISTA DE CANAIS');
  console.log(`👤 Logado como: ${user ? user.username : '?'}`);
  console.log('═'.repeat(70));

  if (cache.guilds.size === 0) {
    console.log('\n⚠️ Você não está em nenhum servidor.\n');
  }

  const channels = cache.listChannels(THREAD_PARENT_TYPES);
  const byGuild = new Map();
  for (const ch of channels) {
    if (!byGuild.has(ch.guildId)) byGuild.set(ch.guildId, []);
    byGuild.get(ch.guildId).push(ch);
  }

  for (const guild of cache.guilds.values()) {
    console.log(`\n🏠 SERVIDOR: ${guild.name}`);
    console.log(`   ID do Servidor: ${guild.id}`);
    console.log('   ' + '─'.repeat(50));

    const chs = byGuild.get(guild.id) || [];
    if (chs.length === 0) {
      console.log('   (Nenhum canal encontrado)');
      continue;
    }

    const byCategory = new Map();
    for (const ch of chs) {
      const cat = ch.categoryName || '📁 Sem Categoria';
      if (!byCategory.has(cat)) byCategory.set(cat, []);
      byCategory.get(cat).push(ch);
    }

    for (const [category, list] of byCategory) {
      console.log(`\n   📁 ${category}`);
      for (const ch of list) {
        console.log(`      ${CHANNEL_TYPE_ICONS[ch.type] || '💬'} #${ch.name}`);
        console.log(`         ID: ${ch.id}`);
      }
    }
  }

  console.log('\n' + '═'.repeat(70));
  console.log('💡 DICA: Copie o ID do canal desejado para o arquivo .env');
  console.log('         Defina: CHANNEL_ID=<id_copiado>');
  console.log('═'.repeat(70) + '\n');

  gateway.destroy();
  process.exit(0);
}

function scheduleSettle() {
  if (settleTimer) clearTimeout(settleTimer);
  settleTimer = setTimeout(printAndExit, SETTLE_MS);
}

gateway.on('dispatch', (t, d) => {
  if (t === 'READY') {
    cache.loadReady(d);
    scheduleSettle();
  } else if (t === 'GUILD_CREATE') {
    cache.handleDispatch(t, d);
    scheduleSettle();
  }
});

gateway.on('fatal', (code, reason) => {
  console.error(`❌ Erro (${code}): ${reason}`);
  process.exit(1);
});

console.log('🔄 Conectando para listar canais...\n');
gateway.connect();
