/**
 * Utilitário para listar todos os canais de texto dos servidores
 * Facilita a obtenção dos IDs dos canais para configuração
 * 
 * Uso: npm run list-channels
 */

require('dotenv').config();
const { Client, ChannelType } = require('discord.js-selfbot-v13');

if (!process.env.DISCORD_TOKEN || process.env.DISCORD_TOKEN.includes('seu_token')) {
  console.error('❌ Configure o DISCORD_TOKEN no arquivo .env primeiro!');
  process.exit(1);
}

const client = new Client({
  checkUpdate: false,
});

client.on('ready', () => {
  console.log('═'.repeat(70));
  console.log('📋 LISTA DE CANAIS DE TEXTO');
  console.log(`👤 Logado como: ${client.user.tag}`);
  console.log('═'.repeat(70));

  // Agrupar canais por servidor
  const servers = client.guilds.cache;

  if (servers.size === 0) {
    console.log('\n⚠️ Você não está em nenhum servidor.\n');
    client.destroy();
    process.exit(0);
  }

  servers.forEach((guild) => {
    console.log(`\n🏠 SERVIDOR: ${guild.name}`);
    console.log(`   ID do Servidor: ${guild.id}`);
    console.log('   ' + '─'.repeat(50));

    // Filtrar apenas canais de texto
    const textChannels = guild.channels.cache.filter(
      ch => ch.type === ChannelType.GuildText ||
        ch.type === ChannelType.GuildForum ||
        ch.type === ChannelType.GuildAnnouncement
    );

    if (textChannels.size === 0) {
      console.log('   (Nenhum canal de texto encontrado)');
      return;
    }

    // Ordenar por categoria
    const channelsByCategory = {};

    textChannels.forEach((channel) => {
      const categoryName = channel.parent?.name || '📁 Sem Categoria';
      if (!channelsByCategory[categoryName]) {
        channelsByCategory[categoryName] = [];
      }

      let typeIcon = '💬';
      if (channel.type === ChannelType.GuildForum) typeIcon = '📋';
      if (channel.type === ChannelType.GuildAnnouncement) typeIcon = '📢';

      channelsByCategory[categoryName].push({
        name: channel.name,
        id: channel.id,
        icon: typeIcon
      });
    });

    // Exibir canais organizados por categoria
    Object.entries(channelsByCategory).forEach(([category, channels]) => {
      console.log(`\n   📁 ${category}`);
      channels.forEach((ch) => {
        console.log(`      ${ch.icon} #${ch.name}`);
        console.log(`         ID: ${ch.id}`);
      });
    });
  });

  console.log('\n' + '═'.repeat(70));
  console.log('💡 DICA: Copie o ID do canal desejado para o arquivo .env');
  console.log('         Defina: CHANNEL_ID=<id_copiado>');
  console.log('═'.repeat(70) + '\n');

  // Encerrar após listar
  client.destroy();
  process.exit(0);
});

client.on('error', (error) => {
  console.error('❌ Erro:', error.message);
  process.exit(1);
});

console.log('🔄 Conectando para listar canais...\n');
client.login(process.env.DISCORD_TOKEN);
