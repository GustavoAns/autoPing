/**
 * Discord AutoPing
 * Monitora um canal específico e responde automaticamente quando um novo tópico é criado.
 * 
 * ⚠️ AVISO: O uso de self-bots viola os Termos de Serviço do Discord.
 * Use por sua conta e risco.
 * 
 * COMANDOS (envie via DM para você mesmo ou digite em qualquer chat):
 *   !autoPing status     - Mostra configuração atual
 *   !autoPing canal ID   - Altera o canal monitorado
 *   !autoPing msg TEXTO  - Altera a mensagem automática
 *   !autoPing listar     - Lista todos os canais disponíveis
 *   !autoPing ajuda      - Mostra todos os comandos
 */

require('dotenv').config();
const { Client } = require('discord.js-selfbot-v13');

// Tipos de canais que suportam threads (strings usadas pela biblioteca)
const THREAD_PARENT_TYPES = [
  'GUILD_TEXT',
  'GUILD_ANNOUNCEMENT',
  'GUILD_NEWS',
  'GUILD_FORUM',
  'GUILD_MEDIA',
  0, 5, 15, 16 // também aceitar números por compatibilidade
];

// ═══════════════════════════════════════════════════════════════════
// CONFIGURAÇÃO DINÂMICA (pode ser alterada via comandos)
// ═══════════════════════════════════════════════════════════════════
const config = {
  channelId: process.env.CHANNEL_ID || '',
  autoMessage: process.env.AUTO_MESSAGE || '',
  waitForMessage: parseInt(process.env.WAIT_FOR_MESSAGE) || 0, // ms para aguardar primeira mensagem (0 = desativado)
  prefix: '!autoPing',
  enabled: true
};

// Validar token obrigatório
if (!process.env.DISCORD_TOKEN || process.env.DISCORD_TOKEN.includes('seu_token')) {
  console.error('❌ Erro: DISCORD_TOKEN não configurado no arquivo .env');
  process.exit(1);
}

const DISCORD_TOKEN = process.env.DISCORD_TOKEN;

// Criar cliente Discord
const client = new Client({
  checkUpdate: false,
});

// ═══════════════════════════════════════════════════════════════════
// FUNÇÕES DE VALIDAÇÃO E UTILITÁRIOS
// ═══════════════════════════════════════════════════════════════════

/**
 * Valida se um canal existe e é acessível
 */
async function validateChannel(channelId) {
  if (!channelId || channelId.length < 17) {
    return { valid: false, error: 'ID do canal inválido. Deve ter 17-19 dígitos.' };
  }

  try {
    const channel = await client.channels.fetch(channelId);

    if (!channel) {
      return { valid: false, error: 'Canal não encontrado. Verifique se o ID está correto.' };
    }

    // Log do tipo de canal para debug
    console.log(`   📋 Tipo do canal: ${channel.type}`);

    // Verificar tipo de canal - aceitar qualquer canal de guild que suporte threads
    if (!THREAD_PARENT_TYPES.includes(channel.type)) {
      return { valid: false, error: `Tipo de canal (${channel.type}) não suportado. Use um canal de texto, fórum, mídia ou anúncios.` };
    }

    // Verificar permissões (usando try-catch pois as flags podem variar entre versões)
    try {
      const permissions = channel.permissionsFor(client.user);
      if (permissions) {
        // Tentar diferentes nomes de permissão (compatibilidade entre versões)
        const canSend = permissions.has('SEND_MESSAGES') || 
                        permissions.has('SendMessages') || 
                        permissions.has(0x800n) || // Bitfield para SEND_MESSAGES
                        permissions.has(2048);     // Número decimal
        if (!canSend) {
          return { valid: false, error: 'Sem permissão para enviar mensagens neste canal.' };
        }
      }
    } catch (permError) {
      // Se der erro na verificação de permissão, ignorar e tentar usar o canal mesmo assim
      console.log(`   ⚠️ Não foi possível verificar permissões: ${permError.message}`);
    }

    return {
      valid: true,
      channel,
      info: `#${channel.name} (${channel.guild.name})`
    };
  } catch (error) {
    if (error.code === 10003) {
      return { valid: false, error: 'Canal não encontrado. O ID pode estar incorreto ou você não tem acesso.' };
    }
    if (error.code === 50001) {
      return { valid: false, error: 'Sem permissão para acessar este canal.' };
    }
    return { valid: false, error: `Erro ao validar canal: ${error.message}` };
  }
}

/**
 * Valida a mensagem automática
 */
function validateMessage(message) {
  if (!message || message.trim().length === 0) {
    return { valid: false, error: 'A mensagem não pode estar vazia.' };
  }
  if (message.length > 2000) {
    return { valid: false, error: 'A mensagem não pode ter mais de 2000 caracteres.' };
  }
  return { valid: true };
}

/**
 * Lista todos os canais de texto disponíveis
 */
function listChannels() {
  const channels = [];

  client.guilds.cache.forEach((guild) => {
    const textChannels = guild.channels.cache.filter(
      ch => THREAD_PARENT_TYPES.includes(ch.type)
    );

    textChannels.forEach((channel) => {
      let typeIcon = '💬';
      if (channel.type === 'GUILD_FORUM' || channel.type === 15) typeIcon = '📋';
      if (channel.type === 'GUILD_ANNOUNCEMENT' || channel.type === 'GUILD_NEWS' || channel.type === 5) typeIcon = '📢';

      channels.push({
        id: channel.id,
        name: channel.name,
        guild: guild.name,
        icon: typeIcon
      });
    });
  });

  return channels;
}

/**
 * Formata a lista de canais para exibição
 */
function formatChannelList(channels, maxPerMessage = 15) {
  if (channels.length === 0) {
    return ['Nenhum canal encontrado.'];
  }

  const messages = [];
  let currentMsg = '📋 **Canais Disponíveis:**\n```\n';
  let count = 0;

  // Agrupar por servidor
  const byGuild = {};
  channels.forEach(ch => {
    if (!byGuild[ch.guild]) byGuild[ch.guild] = [];
    byGuild[ch.guild].push(ch);
  });

  for (const [guild, chs] of Object.entries(byGuild)) {
    currentMsg += `\n🏠 ${guild}\n`;
    for (const ch of chs) {
      const line = `   ${ch.icon} #${ch.name}\n      ID: ${ch.id}\n`;
      if (currentMsg.length + line.length > 1900 || count >= maxPerMessage) {
        currentMsg += '```';
        messages.push(currentMsg);
        currentMsg = '```\n';
        count = 0;
      }
      currentMsg += line;
      count++;
    }
  }

  if (currentMsg.length > 10) {
    currentMsg += '```\n💡 Use `!autoPing canal ID` para definir o canal.';
    messages.push(currentMsg);
  }

  return messages;
}

// ═══════════════════════════════════════════════════════════════════
// COMANDOS
// ═══════════════════════════════════════════════════════════════════

const commands = {
  async status(message) {
    const channelInfo = config.channelId
      ? await validateChannel(config.channelId)
      : { valid: false };

    const waitModeText = config.waitForMessage > 0 
      ? `⏱️ Aguardar 1ª mensagem (até ${config.waitForMessage}ms)` 
      : '⚡ Responder imediatamente';

    const statusText = `
**🤖 Status do AutoPing**

📺 **Canal:** ${channelInfo.valid ? channelInfo.info : (config.channelId || 'Não configurado')}
${channelInfo.valid ? '✅ Canal válido' : '❌ ' + (channelInfo.error || 'Canal não configurado')}

💬 **Mensagem:** \`${config.autoMessage || 'Não configurada'}\`
${config.autoMessage ? '✅ Mensagem válida' : '❌ Mensagem não configurada'}

🕐 **Modo:** ${waitModeText}

⚡ **Status:** ${config.enabled ? '🟢 Ativo' : '🔴 Desativado'}

📝 **Comandos:** \`!autoPing ajuda\`
    `.trim();

    await message.channel.send(statusText);
  },

  async canal(message, args) {
    const newChannelId = args[0];

    if (!newChannelId) {
      await message.channel.send('❌ Use: `!autoPing canal ID_DO_CANAL`\nExemplo: `!autoPing canal 123456789012345678`');
      return;
    }

    await message.channel.send('🔄 Validando canal...');

    const validation = await validateChannel(newChannelId);

    if (!validation.valid) {
      await message.channel.send(`❌ **Erro:** ${validation.error}`);
      return;
    }

    const oldChannel = config.channelId;
    config.channelId = newChannelId;

    await message.channel.send(`✅ **Canal alterado com sucesso!**\n📺 Agora monitorando: ${validation.info}\n\n⚠️ Esta alteração é temporária. Para torná-la permanente, edite o arquivo \`.env\``);

    console.log(`\n🔄 Canal alterado: ${oldChannel} → ${newChannelId}`);
    console.log(`   📺 Novo canal: ${validation.info}\n`);
  },

  async msg(message, args) {
    const newMessage = args.join(' ');

    if (!newMessage) {
      await message.channel.send('❌ Use: `!autoPing msg SUA_MENSAGEM`\nExemplo: `!autoPing msg Olá! Tenho interesse!`');
      return;
    }

    const validation = validateMessage(newMessage);

    if (!validation.valid) {
      await message.channel.send(`❌ **Erro:** ${validation.error}`);
      return;
    }

    const oldMessage = config.autoMessage;
    config.autoMessage = newMessage;

    await message.channel.send(`✅ **Mensagem alterada com sucesso!**\n💬 Nova mensagem: \`${newMessage}\`\n\n⚠️ Esta alteração é temporária. Para torná-la permanente, edite o arquivo \`.env\``);

    console.log(`\n🔄 Mensagem alterada: "${oldMessage}" → "${newMessage}"\n`);
  },

  async listar(message) {
    await message.channel.send('🔄 Carregando lista de canais...');

    const channels = listChannels();
    const messages = formatChannelList(channels);

    for (const msg of messages) {
      await message.channel.send(msg);
    }
  },

  async ajuda(message) {
    const helpText = `
**🤖 Comandos do AutoPing**

\`!autoPing status\` - Mostra a configuração atual
\`!autoPing canal ID\` - Define o canal a ser monitorado
\`!autoPing msg TEXTO\` - Define a mensagem automática
\`!autoPing delay MS\` - Define tempo de espera (0 = imediato)
\`!autoPing listar\` - Lista todos os canais disponíveis
\`!autoPing on\` - Ativa o AutoPing
\`!autoPing off\` - Desativa o AutoPing
\`!autoPing ajuda\` - Mostra esta mensagem

**Exemplos:**
\`!autoPing canal 123456789012345678\`
\`!autoPing msg 222/555/666-FB/666-Rep\`
\`!autoPing delay 5000\` - Aguarda até 5s pela 1ª mensagem
\`!autoPing delay 0\` - Responde imediatamente
    `.trim();

    await message.channel.send(helpText);
  },

  async on(message) {
    config.enabled = true;
    await message.channel.send('✅ AutoPing **ativado**!');
    console.log('\n🟢 AutoPing ativado via comando\n');
  },

  async delay(message, args) {
    const newDelay = parseInt(args[0]);

    if (args.length === 0 || isNaN(newDelay) || newDelay < 0) {
      await message.channel.send(`❌ Use: \`!autoPing delay MS\`\n\n**Exemplos:**\n\`!autoPing delay 5000\` - Aguarda até 5 segundos pela 1ª mensagem\n\`!autoPing delay 0\` - Responde imediatamente (padrão)\n\n**Atual:** ${config.waitForMessage}ms`);
      return;
    }

    const oldDelay = config.waitForMessage;
    config.waitForMessage = newDelay;

    if (newDelay === 0) {
      await message.channel.send(`✅ **Modo alterado!**\n⚡ Agora responde **imediatamente** quando um tópico é criado.\n\n⚠️ Esta alteração é temporária. Para torná-la permanente, edite o arquivo \`.env\``);
    } else {
      await message.channel.send(`✅ **Modo alterado!**\n⏱️ Agora aguarda até **${newDelay}ms** pela primeira mensagem do criador antes de responder.\n\n⚠️ Esta alteração é temporária. Para torná-la permanente, edite o arquivo \`.env\``);
    }

    console.log(`\n🔄 Delay alterado: ${oldDelay}ms → ${newDelay}ms\n`);
  },

  async off(message) {
    config.enabled = false;
    await message.channel.send('🔴 AutoPing **desativado**!');
    console.log('\n🔴 AutoPing desativado via comando\n');
  }
};

// ═══════════════════════════════════════════════════════════════════
// EVENTOS DO DISCORD
// ═══════════════════════════════════════════════════════════════════

// Evento: Bot conectado
client.on('ready', async () => {
  console.log('═'.repeat(50));
  console.log('🚀 Discord AutoPing Iniciado!');
  console.log('═'.repeat(50));
  console.log(`👤 Logado como: ${client.user.tag}`);

  // Validar canal inicial
  if (config.channelId) {
    const validation = await validateChannel(config.channelId);
    if (validation.valid) {
      console.log(`📺 Monitorando: ${validation.info}`);
    } else {
      console.log(`⚠️ Canal configurado mas com problema: ${validation.error}`);
      console.log(`   Use !autoPing canal ID para configurar um canal válido`);
    }
  } else {
    console.log('⚠️ Nenhum canal configurado. Use !autoPing canal ID');
  }

  console.log(`💬 Mensagem: "${config.autoMessage || 'Não configurada'}"`);
  console.log(`🕐 Modo: ${config.waitForMessage > 0 ? `Aguardar até ${config.waitForMessage}ms pela 1ª mensagem` : 'Resposta imediata'}`);
  console.log('═'.repeat(50));
  console.log('📝 Comandos disponíveis: !autoPing ajuda');
  console.log('⏳ Aguardando criação de novos tópicos...\n');
});

// Evento: Mensagem recebida (para comandos)
client.on('messageCreate', async (message) => {
  // Ignorar mensagens de outros usuários
  if (message.author.id !== client.user.id) return;

  // Verificar se é um comando
  if (!message.content.startsWith(config.prefix)) return;

  const args = message.content.slice(config.prefix.length).trim().split(/\s+/);
  const commandName = args.shift()?.toLowerCase();

  if (commandName && commands[commandName]) {
    try {
      await commands[commandName](message, args);
      // Deletar mensagem de comando após 2 segundos (opcional, para não poluir)
      setTimeout(() => message.delete().catch(() => { }), 2000);
    } catch (error) {
      console.error(`❌ Erro ao executar comando ${commandName}:`, error.message);
      await message.channel.send(`❌ Erro ao executar comando: ${error.message}`);
    }
  }
});

// Evento: Novo tópico (thread) criado
client.on('threadCreate', async (thread, newlyCreated) => {
  // Verificar se está ativado
  if (!config.enabled) return;

  // Ignorar threads que não foram recém-criadas
  if (!newlyCreated) return;

  // Verificar se é no canal que estamos monitorando
  if (thread.parentId !== config.channelId) return;

  console.log(`\n🆕 Novo tópico detectado!`);
  console.log(`   📌 Nome: ${thread.name}`);
  console.log(`   🆔 ID: ${thread.id}`);
  console.log(`   👤 Criador: ${thread.ownerId}`);

  // Validar mensagem antes de enviar
  if (!config.autoMessage) {
    console.error('   ❌ Mensagem automática não configurada!\n');
    return;
  }

  try {
    // Entrar no tópico (necessário para enviar mensagem)
    if (!thread.joined) {
      await thread.join();
    }

    const startTime = Date.now();

    // Se waitForMessage > 0, aguardar a primeira mensagem do criador
    if (config.waitForMessage > 0) {
      console.log(`   ⏳ Aguardando primeira mensagem (até ${config.waitForMessage}ms)...`);
      
      // Criar um collector para aguardar a primeira mensagem
      const filter = (msg) => msg.author.id === thread.ownerId;
      
      try {
        // Aguardar a primeira mensagem do criador do tópico
        const collected = await thread.awaitMessages({
          filter,
          max: 1,
          time: config.waitForMessage,
          errors: ['time']
        });
        
        const firstMessage = collected.first();
        console.log(`   📨 Primeira mensagem detectada de ${firstMessage.author.tag}`);
      } catch (timeoutError) {
        // Timeout - nenhuma mensagem recebida no tempo limite
        console.log(`   ⏰ Timeout - enviando mensagem mesmo assim`);
      }
    }

    // Enviar mensagem
    await thread.send(config.autoMessage);
    const responseTime = Date.now() - startTime;

    console.log(`   ✅ Mensagem enviada com sucesso!`);
    console.log(`   ⚡ Tempo de resposta: ${responseTime}ms\n`);
  } catch (error) {
    // Tratamento detalhado de erros
    let errorMsg = error.message;

    if (error.code === 50001) {
      errorMsg = 'Sem permissão para acessar este tópico.';
    } else if (error.code === 50013) {
      errorMsg = 'Sem permissão para enviar mensagens neste tópico.';
    } else if (error.code === 10008) {
      errorMsg = 'Mensagem não encontrada ou deletada.';
    } else if (error.code === 50035) {
      errorMsg = 'Mensagem inválida (muito longa ou formato incorreto).';
    } else if (error.code === 40001) {
      errorMsg = 'Conta não autorizada. Verifique o token.';
    } else if (error.code === 10003) {
      errorMsg = 'Canal/Tópico não encontrado.';
    }

    console.error(`   ❌ Erro: ${errorMsg}`);
    console.error(`   📋 Código: ${error.code || 'N/A'}\n`);
  }
});

// Evento: Erro de conexão
client.on('error', (error) => {
  console.error('❌ Erro de conexão:', error.message);

  if (error.code === 'TOKEN_INVALID') {
    console.error('   💡 O token está inválido. Gere um novo token.');
  }
});

// Evento: Desconectado
client.on('disconnect', () => {
  console.log('⚠️ Desconectado do Discord. Tentando reconectar...');
});

// Evento: Reconectando
client.on('reconnecting', () => {
  console.log('🔄 Reconectando ao Discord...');
});

// Tratamento de erros não capturados
process.on('unhandledRejection', (error) => {
  console.error('❌ Erro não tratado:', error.message);
});

// Tratamento de encerramento gracioso
process.on('SIGINT', () => {
  console.log('\n👋 Encerrando AutoPing...');
  client.destroy();
  process.exit(0);
});

// Conectar ao Discord
console.log('🔄 Conectando ao Discord...\n');
client.login(DISCORD_TOKEN);
