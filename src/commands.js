/**
 * Comandos `!autoPing ...` enviados pela própria conta em qualquer chat.
 * Todos respondem no chat e logam no terminal. A mensagem do comando é
 * apagada após 2s.
 */

const { validateMessage, isSnowflake, THREAD_PARENT_TYPES, CHANNEL_TYPE_ICONS } = require('./config');

const DELETE_COMMAND_AFTER_MS = 2000;

function createChannelValidator({ rest, cache }) {
  /**
   * Valida se o canal existe, é acessível e suporta tópicos.
   */
  return async function validateChannel(channelId) {
    if (!isSnowflake(channelId)) {
      return { valid: false, error: 'ID do canal inválido. Deve ter 17-20 dígitos.' };
    }
    try {
      const channel = await rest.getChannel(channelId);
      if (!THREAD_PARENT_TYPES.has(channel.type)) {
        return { valid: false, error: `Tipo de canal (${channel.type}) não suportado. Use um canal de texto, fórum, mídia ou anúncios.` };
      }
      const guildName = cache.getGuildName(channel.guild_id) || channel.guild_id;
      return { valid: true, channel, guildId: channel.guild_id, info: `#${channel.name} (${guildName})` };
    } catch (error) {
      if (error.code === 10003 || error.status === 404) {
        return { valid: false, error: 'Canal não encontrado. O ID pode estar incorreto ou você não tem acesso.' };
      }
      if (error.code === 50001 || error.status === 403) {
        return { valid: false, error: 'Sem permissão para acessar este canal.' };
      }
      return { valid: false, error: `Erro ao validar canal: ${error.message}` };
    }
  };
}

function formatChannelList(channels, maxPerMessage = 15) {
  if (channels.length === 0) return ['Nenhum canal encontrado.'];

  const messages = [];
  let current = '📋 **Canais Disponíveis:**\n```\n';
  let count = 0;

  const byGuild = new Map();
  for (const ch of channels) {
    if (!byGuild.has(ch.guildName)) byGuild.set(ch.guildName, []);
    byGuild.get(ch.guildName).push(ch);
  }

  for (const [guild, chs] of byGuild) {
    current += `\n🏠 ${guild}\n`;
    for (const ch of chs) {
      const line = `   ${CHANNEL_TYPE_ICONS[ch.type] || '💬'} #${ch.name}\n      ID: ${ch.id}\n`;
      if (current.length + line.length > 1900 || count >= maxPerMessage) {
        messages.push(current + '```');
        current = '```\n';
        count = 0;
      }
      current += line;
      count++;
    }
  }

  messages.push(current + '```\n💡 Use `!autoPing canal ID` para definir o canal.');
  return messages;
}

const HELP_TEXT = `
**🤖 Comandos do AutoPing**

\`!autoPing status\` - Mostra a configuração atual
\`!autoPing canal ID\` - Define o canal a ser monitorado
\`!autoPing msg TEXTO\` - Define a mensagem automática
\`!autoPing delay MS\` - Tempo máx. aguardando a 1ª msg do criador
\`!autoPing listar\` - Lista todos os canais disponíveis
\`!autoPing on\` - Ativa o AutoPing
\`!autoPing off\` - Desativa o AutoPing (cancela tópicos pendentes)
\`!autoPing ajuda\` - Mostra esta mensagem

**Exemplos:**
\`!autoPing canal 123456789012345678\`
\`!autoPing msg 222/555/666-FB/666-Rep\`
\`!autoPing delay 5000\` - Aguarda até 5s pela 1ª mensagem do criador
`.trim();

function modeText(config) {
  const extra = config.humanizedDelay > 0 ? ` +${config.humanizedDelay}ms` : '';
  return `⚡ Responder logo após a 1ª mensagem do criador${extra} (timeout ${config.waitForMessage}ms)`;
}

function createCommands({ config, rest, cache, validateChannel, getSelfId, printStatus }) {
  const commands = {
    async status() {
      const ch = config.channelId ? await validateChannel(config.channelId) : { valid: false };
      const channelLine = ch.valid ? ch.info : (config.channelId || 'Não configurado');
      console.log(`   📺 Canal: ${channelLine} | 💬 "${config.autoMessage || 'Não configurada'}" | ${config.enabled ? '🟢 Ativo' : '🔴 Desativado'}`);
      return `
**🤖 Status do AutoPing**

📺 **Canal:** ${channelLine}
${ch.valid ? '✅ Canal válido' : '❌ ' + (ch.error || 'Canal não configurado')}

💬 **Mensagem:** \`${config.autoMessage || 'Não configurada'}\`
${config.autoMessage ? '✅ Mensagem válida' : '❌ Mensagem não configurada'}

🕐 **Modo:** ${modeText(config)}

⚡ **Status:** ${config.enabled ? '🟢 Ativo' : '🔴 Desativado'}

📝 **Comandos:** \`!autoPing ajuda\`
`.trim();
    },

    async canal(args) {
      const id = args[0];
      if (!id) return '❌ Informe o ID do canal. Uso: `!autoPing canal ID_DO_CANAL`';
      console.log(`   🔄 Validando canal: ${id}`);
      const v = await validateChannel(id);
      if (!v.valid) return `❌ ${v.error}`;
      const old = config.channelId;
      config.set('channelId', id); // listener em index.js cancela pendentes e assina o canal
      console.log(`   ✅ Canal alterado: ${old || 'nenhum'} → ${id} (${v.info})`);
      await printStatus();
      return `✅ Canal alterado para **${v.info}**`;
    },

    async msg(args) {
      const text = args.join(' ');
      if (!text) return '❌ Informe a mensagem. Uso: `!autoPing msg SUA_MENSAGEM`';
      const v = validateMessage(text);
      if (!v.valid) return `❌ ${v.error}`;
      const old = config.autoMessage;
      config.set('autoMessage', text);
      console.log(`   ✅ Mensagem alterada: "${old || 'nenhuma'}" → "${text}"`);
      await printStatus();
      return `✅ Mensagem alterada para \`${text}\``;
    },

    async delay(args) {
      const ms = parseInt(args[0], 10);
      if (args.length === 0 || Number.isNaN(ms) || ms < 500) {
        return `❌ Valor inválido (mínimo 500). Atual: ${config.waitForMessage}ms. Uso: \`!autoPing delay MS\` (ex: 5000)`;
      }
      const old = config.waitForMessage;
      config.set('waitForMessage', ms);
      console.log(`   ✅ Delay alterado: ${old}ms → ${ms}ms`);
      await printStatus();
      return `✅ Modo: ${modeText(config)}`;
    },

    async on() {
      config.set('enabled', true);
      console.log('   🟢 AutoPing ATIVADO');
      await printStatus();
      return '🟢 AutoPing **ativado**';
    },

    async off() {
      config.set('enabled', false); // listener em index.js cancela tópicos pendentes
      console.log('   🔴 AutoPing DESATIVADO');
      await printStatus();
      return '🔴 AutoPing **desativado**';
    },

    async listar() {
      const channels = cache.listChannels(THREAD_PARENT_TYPES);
      console.log(`   ✅ ${channels.length} canais encontrados`);
      for (const ch of channels) {
        console.log(`      ${CHANNEL_TYPE_ICONS[ch.type] || '💬'} ${ch.guildName} > #${ch.name} (${ch.id})`);
      }
      return formatChannelList(channels);
    },

    async ajuda() {
      return HELP_TEXT;
    },
  };

  /**
   * Processa um MESSAGE_CREATE. Ignora mensagens de outros usuários.
   */
  async function handleMessage(d) {
    if (!d.author || d.author.id !== getSelfId()) return;
    if (typeof d.content !== 'string' || !d.content.startsWith(config.prefix)) return;

    const args = d.content.slice(config.prefix.length).trim().split(/\s+/).filter(Boolean);
    const name = (args.shift() || '').toLowerCase();
    const command = commands[name];
    if (!command) return;

    console.log(`\n📋 Comando: ${config.prefix} ${name}`);

    let reply;
    try {
      reply = await command(args);
    } catch (error) {
      console.error(`❌ Erro ao executar comando ${name}: ${error.message}`);
      reply = `❌ Erro ao executar comando: ${error.message}`;
    }

    const replies = Array.isArray(reply) ? reply : [reply];
    for (const text of replies) {
      if (!text) continue;
      try {
        await rest.sendMessage(d.channel_id, text);
      } catch (error) {
        console.error(`   ❌ Erro ao responder comando: ${error.message}`);
        break;
      }
    }

    setTimeout(() => rest.deleteMessage(d.channel_id, d.id).catch(() => {}), DELETE_COMMAND_AFTER_MS).unref();
  }

  return { handleMessage, commands };
}

module.exports = { createCommands, createChannelValidator, formatChannelList, modeText };
