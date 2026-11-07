/**
 * Cache mínimo de servidores e canais, alimentado pelo gateway
 * (READY, GUILD_CREATE/UPDATE/DELETE, CHANNEL_CREATE/UPDATE/DELETE).
 * Usado para listar canais e mostrar nomes em logs/status.
 *
 * Os payloads de conta de usuário variam conforme as capabilities
 * (ex.: nome do servidor pode vir em `guild.properties.name`), por isso o
 * parse é defensivo.
 */

const CATEGORY_TYPE = 4;

class Cache {
  constructor() {
    this.guilds = new Map(); // guildId -> { id, name, channels: Map<channelId, channel> }
  }

  loadReady(d) {
    this.guilds.clear();
    for (const g of d.guilds || []) this.upsertGuild(g);
  }

  upsertGuild(g) {
    if (!g || !g.id || g.unavailable) return;
    const existing = this.guilds.get(g.id);
    const name = (g.properties && g.properties.name) || g.name || (existing && existing.name) || g.id;
    const guild = existing || { id: g.id, name, channels: new Map() };
    guild.name = name;
    if (Array.isArray(g.channels)) {
      guild.channels.clear();
      for (const ch of g.channels) this._setChannel(guild, ch);
    }
    this.guilds.set(g.id, guild);
  }

  _setChannel(guild, ch) {
    guild.channels.set(ch.id, {
      id: ch.id,
      name: ch.name,
      type: ch.type,
      parentId: ch.parent_id || null,
      guildId: guild.id,
    });
  }

  handleDispatch(t, d) {
    switch (t) {
      case 'GUILD_CREATE':
      case 'GUILD_UPDATE':
        this.upsertGuild(d);
        break;
      case 'GUILD_DELETE':
        if (!d.unavailable) this.guilds.delete(d.id);
        break;
      case 'CHANNEL_CREATE':
      case 'CHANNEL_UPDATE': {
        const guild = d.guild_id && this.guilds.get(d.guild_id);
        if (guild) this._setChannel(guild, d);
        break;
      }
      case 'CHANNEL_DELETE': {
        const guild = d.guild_id && this.guilds.get(d.guild_id);
        if (guild) guild.channels.delete(d.id);
        break;
      }
      default:
        break;
    }
  }

  getGuildName(guildId) {
    const g = this.guilds.get(guildId);
    return g ? g.name : null;
  }

  getChannel(channelId) {
    for (const guild of this.guilds.values()) {
      const ch = guild.channels.get(channelId);
      if (ch) return { ...ch, guildName: guild.name };
    }
    return null;
  }

  /**
   * Lista canais cujo tipo está em `types`, com nome da categoria.
   */
  listChannels(types) {
    const result = [];
    for (const guild of this.guilds.values()) {
      for (const ch of guild.channels.values()) {
        if (!types.has(ch.type)) continue;
        const category = ch.parentId && guild.channels.get(ch.parentId);
        result.push({
          ...ch,
          guildName: guild.name,
          categoryName: category && category.type === CATEGORY_TYPE ? category.name : null,
        });
      }
    }
    return result;
  }
}

module.exports = { Cache };
