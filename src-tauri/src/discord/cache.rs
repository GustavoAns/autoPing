//! Cache mínimo de servidores e canais vindos do gateway (READY, GUILD_*,
//! CHANNEL_*). Usado para listar canais na config e mostrar nomes.
//! Parse defensivo: com capabilities, o nome pode vir em `properties.name`.

use crate::config::THREAD_PARENT_TYPES;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const CATEGORY_TYPE: u8 = 4;

#[derive(Deserialize)]
pub struct GuildPayload {
    pub id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    properties: Option<GuildProps>,
    #[serde(default)]
    channels: Option<Vec<ChannelPayload>>,
    #[serde(default)]
    unavailable: Option<bool>,
}

#[derive(Deserialize)]
struct GuildProps {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct ChannelPayload {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: u8,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub guild_id: Option<String>,
}

#[derive(Deserialize)]
pub struct ReadyPayload {
    pub user: ReadyUser,
    #[serde(default)]
    pub guilds: Vec<GuildPayload>,
}

#[derive(Deserialize, Clone)]
pub struct ReadyUser {
    pub id: String,
    #[serde(default)]
    pub username: String,
}

#[derive(Deserialize)]
pub struct GuildDeletePayload {
    pub id: String,
    #[serde(default)]
    pub unavailable: Option<bool>,
}

struct Guild {
    name: String,
    channels: HashMap<String, ChannelPayload>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelEntry {
    pub id: String,
    pub name: String,
    pub kind: u8,
    pub guild_id: String,
    pub guild_name: String,
    pub category: Option<String>,
}

#[derive(Default)]
pub struct Cache {
    guilds: HashMap<String, Guild>,
}

impl Cache {
    pub fn load_ready(&mut self, guilds: Vec<GuildPayload>) {
        self.guilds.clear();
        for g in guilds {
            self.upsert_guild(g);
        }
    }

    pub fn upsert_guild(&mut self, g: GuildPayload) {
        if g.unavailable == Some(true) {
            return;
        }
        let name = g
            .properties
            .and_then(|p| p.name)
            .or(g.name)
            .or_else(|| self.guilds.get(&g.id).map(|x| x.name.clone()))
            .unwrap_or_else(|| g.id.clone());
        let entry = self.guilds.entry(g.id).or_insert_with(|| Guild { name: String::new(), channels: HashMap::new() });
        entry.name = name;
        if let Some(chs) = g.channels {
            entry.channels = chs.into_iter().map(|c| (c.id.clone(), c)).collect();
        }
    }

    pub fn delete_guild(&mut self, d: GuildDeletePayload) {
        if d.unavailable != Some(true) {
            self.guilds.remove(&d.id);
        }
    }

    pub fn upsert_channel(&mut self, c: ChannelPayload) {
        if let Some(g) = c.guild_id.as_ref().and_then(|id| self.guilds.get_mut(id)) {
            g.channels.insert(c.id.clone(), c);
        }
    }

    pub fn delete_channel(&mut self, c: ChannelPayload) {
        if let Some(g) = c.guild_id.as_ref().and_then(|id| self.guilds.get_mut(id)) {
            g.channels.remove(&c.id);
        }
    }

    pub fn guild_name(&self, guild_id: &str) -> Option<String> {
        self.guilds.get(guild_id).map(|g| g.name.clone())
    }

    /// Canais que podem ter tópicos, ordenados por servidor/categoria/nome.
    pub fn list_channels(&self) -> Vec<ChannelEntry> {
        let mut out = Vec::new();
        for (gid, g) in &self.guilds {
            for c in g.channels.values() {
                if !THREAD_PARENT_TYPES.contains(&c.kind) {
                    continue;
                }
                let category = c
                    .parent_id
                    .as_ref()
                    .and_then(|p| g.channels.get(p))
                    .filter(|p| p.kind == CATEGORY_TYPE)
                    .and_then(|p| p.name.clone());
                out.push(ChannelEntry {
                    id: c.id.clone(),
                    name: c.name.clone().unwrap_or_default(),
                    kind: c.kind,
                    guild_id: gid.clone(),
                    guild_name: g.name.clone(),
                    category,
                });
            }
        }
        out.sort_by(|a, b| {
            (a.guild_name.to_lowercase(), a.category.clone().unwrap_or_default().to_lowercase(), a.name.to_lowercase()).cmp(&(
                b.guild_name.to_lowercase(),
                b.category.clone().unwrap_or_default().to_lowercase(),
                b.name.to_lowercase(),
            ))
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ready_guild_with_properties_name() {
        let json = r#"{"user":{"id":"1","username":"eu"},"guilds":[
            {"id":"G","properties":{"name":"Bunker"},"channels":[
                {"id":"CAT","type":4,"name":"Testes"},
                {"id":"C1","type":0,"name":"area-de-testes","parent_id":"CAT"},
                {"id":"V","type":2,"name":"voz"},
                {"id":"F","type":15,"name":"vagas"}
            ]},
            {"id":"X","unavailable":true}
        ]}"#;
        let r: ReadyPayload = serde_json::from_str(json).unwrap();
        let mut c = Cache::default();
        c.load_ready(r.guilds);
        let list = c.list_channels();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].guild_name, "Bunker");
        let t = list.iter().find(|x| x.id == "C1").unwrap();
        assert_eq!(t.category.as_deref(), Some("Testes"));
    }
}
