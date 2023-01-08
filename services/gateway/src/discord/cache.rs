use fred::{
    pool::RedisPool,
    prelude::{HashesInterface, KeysInterface},
    types::{RedisConfig, RedisKey, RedisValue},
};
use prost::Message;
use tracing::{debug, info};
use twilight_gateway::Event;
use twilight_model::{
    channel::{Channel, ChannelType},
    gateway::payload::incoming::{GuildCreate, MessageCreate, ThreadListSync},
    guild::{Guild, PartialGuild, PartialMember, Role},
    id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
    id::Id,
    user::User,
};

use crate::config;
use crate::util::proto;
use crate::util::twilight;

#[derive(Clone)]
pub struct RedisCache {
    redis: RedisPool,
    own_user_id: Id<UserMarker>,
}

pub async fn init() -> anyhow::Result<RedisCache> {
    let cfg = config::config().await?;
    let redis = connect_to_redis(&cfg.gateway_redis_addr).await?;
    Ok(RedisCache {
        own_user_id: Id::<UserMarker>::new(cfg.client_id),
        redis,
    })
}

// Myriad.Cache.DiscordCacheExtensions impl
impl RedisCache {
    pub async fn handle_event(&mut self, event: Event) -> anyhow::Result<()> {
        self.try_update_self_member(event.clone()).await?;

        match event {
            Event::GuildCreate(gc) => self.save_guild_create(*gc).await,
            Event::GuildUpdate(gu) => self.save_partial_guild(gu.0).await,
            Event::GuildDelete(gd) => self.remove_guild(gd.id).await,
            Event::ChannelCreate(cc) => self.save_channel(cc.0).await,
            Event::ChannelUpdate(cu) => self.save_channel(cu.0).await,
            Event::ChannelDelete(cd) => self.remove_channel(cd.id).await,
            Event::RoleCreate(grc) => self.save_role(grc.guild_id, grc.role).await,
            Event::RoleUpdate(gru) => self.save_role(gru.guild_id, gru.role).await,
            Event::RoleDelete(grd) => self.remove_role(grd.guild_id, grd.role_id).await,
            Event::ReactionAdd(mra) if mra.guild_id.is_none() => {
                self.save_dm_channel_stub(mra.channel_id).await
            }
            Event::MessageCreate(mc) => self.save_message_create(*mc).await,
            Event::MessageUpdate(mu) if mu.guild_id.is_none() => {
                self.save_dm_channel_stub(mu.channel_id).await
            }
            Event::MessageDelete(md) if md.guild_id.is_none() => {
                self.save_dm_channel_stub(md.channel_id).await
            }
            Event::MessageDeleteBulk(mdb) if mdb.guild_id.is_none() => {
                self.save_dm_channel_stub(mdb.channel_id).await
            }
            Event::ThreadCreate(tc) => self.save_channel(tc.0).await,
            Event::ThreadUpdate(tu) => self.save_channel(tu.0).await,
            Event::ThreadDelete(td) => self.remove_channel(td.id).await,
            Event::ThreadListSync(tls) => self.save_thread_list_sync(tls).await,
            _ => Ok(()),
        }
    }

    async fn try_update_self_member(&mut self, event: Event) -> anyhow::Result<()> {
        let data: Option<(Id<GuildMarker>, PartialMember)> = match event {
            // if (evt is GuildCreateEvent gc) return cache.SaveSelfMember(gc.Id, gc.Members.FirstOrDefault(m => m.User.Id == userId)!);
            Event::GuildCreate(gc) => Some((
                gc.id,
                twilight::member_to_partial_member(
                    gc.members
                        .clone()
                        .into_iter()
                        .find(|m| m.user.id == self.own_user_id)
                        .expect("could not find self member in guild create!"),
                ),
            )),

            Event::MessageCreate(mc)
                if !mc.member.is_none() && mc.author.id == self.own_user_id =>
            {
                Some((mc.guild_id.unwrap(), mc.member.clone().unwrap()))
            }

            Event::MemberAdd(gma) if gma.user.id == self.own_user_id => {
                Some((gma.guild_id, twilight::member_to_partial_member(gma.0)))
            }
            Event::MemberUpdate(gmu) if gmu.user.id == self.own_user_id => Some((
                gmu.guild_id,
                twilight::member_update_to_partial_member(*gmu),
            )),

            _ => None,
        };

        if let Some((guild_id, member)) = data {
            self.save_self_member(guild_id, member).await?;
        }

        Ok(())
    }

    async fn save_message_create(&mut self, event: MessageCreate) -> anyhow::Result<()> {
        debug!("save_message_create");
        if let None = event.guild_id {
            self.save_dm_channel_stub(event.channel_id).await?;
        }

        self.save_user(event.author.clone()).await?;
        for mention in &event.mentions {
            self.save_user(twilight::mention_to_user(mention.clone()))
                .await?;
        }

        Ok(())
    }

    async fn save_thread_list_sync(&mut self, event: ThreadListSync) -> anyhow::Result<()> {
        debug!("save_thread_list_sync");
        for thread in event.threads {
            self.save_channel(thread).await?;
        }

        Ok(())
    }
}

// custom
impl RedisCache {
    async fn save_guild_create(&mut self, event: GuildCreate) -> anyhow::Result<()> {
        debug!("save_guild_create {}", event.clone().id.get());
        // clear existing data
        self.redis
            .del("discord:cache:guild_channels".to_owned() + &event.id.get().to_string())
            .await?;
        self.redis
            .del("discord:cache:guild_roles".to_owned() + &event.id.get().to_string())
            .await?;

        self.save_guild(event.clone().0).await?;

        if event.channels.len() > 0 {
            self.redis
                .hset(
                    "discord:cache:channels",
                    event
                        .channels
                        .clone()
                        .into_iter()
                        .map(|mut c| {
                            c.guild_id = Some(event.id);
                            (
                                RedisKey::from(c.id.get()),
                                RedisValue::Bytes(
                                    proto::CachedChannel::from(c).encode_to_vec().into(),
                                ),
                            )
                        })
                        .collect::<Vec<(RedisKey, RedisValue)>>(),
                )
                .await?;

            self.redis
                .hset(
                    "discord:cache:guild_channels:".to_owned() + &event.id.get().to_string(),
                    event
                        .channels
                        .clone()
                        .into_iter()
                        .map(|c| (RedisKey::from(c.id.get()), RedisValue::Boolean(true)))
                        .collect::<Vec<(RedisKey, RedisValue)>>(),
                )
                .await?;
        }

        if event.threads.len() > 0 {
            self.redis
                .hset(
                    "discord:cache:channels",
                    event
                        .threads
                        .clone()
                        .into_iter()
                        .map(|mut c| {
                            c.guild_id = Some(event.id);
                            (
                                RedisKey::from(c.id.get()),
                                RedisValue::Bytes(
                                    proto::CachedChannel::from(c).encode_to_vec().into(),
                                ),
                            )
                        })
                        .collect::<Vec<(RedisKey, RedisValue)>>(),
                )
                .await?;

            self.redis
                .hset(
                    "discord:cache:guild_channels:".to_owned() + &event.id.get().to_string(),
                    event
                        .threads
                        .clone()
                        .into_iter()
                        .map(|c| (RedisKey::from(c.id.get()), RedisValue::Boolean(true)))
                        .collect::<Vec<(RedisKey, RedisValue)>>(),
                )
                .await?;
        }

        if event.roles.len() > 0 {
            self.redis
                .hset(
                    "discord:cache:roles",
                    event
                        .roles
                        .clone()
                        .into_iter()
                        .map(|c| {
                            (
                                RedisKey::from(c.id.get()),
                                RedisValue::Bytes(
                                    proto::CachedRole::from(c).encode_to_vec().into(),
                                ),
                            )
                        })
                        .collect::<Vec<(RedisKey, RedisValue)>>(),
                )
                .await?;

            self.redis
                .hset(
                    "discord:cache:guild_roles:".to_owned() + &event.id.get().to_string(),
                    event
                        .roles
                        .clone()
                        .into_iter()
                        .map(|c| (RedisKey::from(c.id.get()), RedisValue::Boolean(true)))
                        .collect::<Vec<(RedisKey, RedisValue)>>(),
                )
                .await?;
        }

        debug!("save_guild_create end {}", event.clone().id.get());
        Ok(())
    }
}

// Myriad.Cache.IDiscordCache impl
impl RedisCache {
    async fn save_guild(&mut self, guild: Guild) -> anyhow::Result<()> {
        debug!("save_guild");
        self.redis
            .hset(
                "discord:cache:guilds",
                (
                    RedisKey::from(guild.id.get()),
                    RedisValue::Bytes(proto::CachedGuild::from(guild).encode_to_vec().into()),
                ),
            )
            .await?;

        Ok(())
    }

    // aaaaa
    async fn save_partial_guild(&mut self, guild: PartialGuild) -> anyhow::Result<()> {
        debug!("save_partial_guild");
        self.redis
            .hset(
                "discord:cache:guilds",
                (
                    RedisKey::from(guild.id.get()),
                    RedisValue::Bytes(proto::CachedGuild::from(guild).encode_to_vec().into()),
                ) as (RedisKey, RedisValue),
            )
            .await?;

        Ok(())
    }

    async fn save_channel(&mut self, channel: Channel) -> anyhow::Result<()> {
        debug!("save_channel");
        self.redis
            .hset(
                "discord:cache:channels",
                (
                    RedisKey::from(channel.id.get()),
                    RedisValue::Bytes(proto::CachedChannel::from(channel).encode_to_vec().into()),
                ) as (RedisKey, RedisValue),
            )
            .await?;

        Ok(())
    }

    async fn save_user(&mut self, user: User) -> anyhow::Result<()> {
        debug!("save_user");

        self.redis
            .hset(
                "discord:cache:users",
                (
                    RedisKey::try_from(user.id.get()).expect("failed to create redis key"),
                    RedisValue::Bytes(proto::CachedUser::from(user.clone()).encode_to_vec().into()),
                ),
            )
            .await?;

        Ok(())
    }

    async fn save_self_member(
        &mut self,
        guild_id: Id<GuildMarker>,
        member: PartialMember,
    ) -> anyhow::Result<()> {
        debug!("save_self_member");
        self.redis
            .hset(
                "discord:cache:members",
                (
                    RedisKey::from(guild_id.get()),
                    RedisValue::Bytes(
                        proto::CachedGuildMember {
                            roles: member.roles.into_iter().map(|r| r.get()).collect(),
                        }
                        .encode_to_vec()
                        .into(),
                    ),
                ),
            )
            .await?;

        Ok(())
    }

    async fn save_role(&mut self, guild_id: Id<GuildMarker>, role: Role) -> anyhow::Result<()> {
        debug!("save_role");
        self.redis
            .hset(
                "discord:cache:roles",
                (
                    RedisKey::from(role.clone().id.get()),
                    RedisValue::Bytes(proto::CachedRole::from(role.clone()).encode_to_vec().into()),
                ),
            )
            .await?;

        self.redis
            .hset(
                "discord:cache:guild_roles:".to_owned() + &guild_id.get().to_string(),
                (RedisKey::from(role.id.get()), RedisValue::Boolean(true)),
            )
            .await?;

        Ok(())
    }

    async fn save_dm_channel_stub(&mut self, channel_id: Id<ChannelMarker>) -> anyhow::Result<()> {
        debug!("save_dm_channel_stub");
        if !self
            .redis
            .hexists("discord:cache:channels", channel_id.clone().get())
            .await?
        {
            self.redis
                .hset(
                    "discord:cache:channels",
                    (
                        RedisKey::from(channel_id.get()),
                        RedisValue::Bytes(
                            proto::CachedChannel {
                                id: channel_id.get(),
                                r#type: u8::from(ChannelType::Private) as _,
                                ..Default::default()
                            }
                            .encode_to_vec()
                            .into(),
                        ),
                    ),
                )
                .await?;
        }

        Ok(())
    }

    async fn remove_guild(&mut self, guild_id: Id<GuildMarker>) -> anyhow::Result<()> {
        debug!("remove_guild");
        self.redis
            .hdel("discord:cache:guilds", guild_id.get())
            .await?;
        self.redis
            .del("discord:cache:guild_channels:".to_owned() + &guild_id.get().to_string())
            .await?;
        self.redis
            .del("discord:cache:guild_roles:".to_owned() + &guild_id.get().to_string())
            .await?;
        Ok(())
    }

    async fn remove_channel(&mut self, channel_id: Id<ChannelMarker>) -> anyhow::Result<()> {
        debug!("remove_channel");
        if let Some(old_channel) = self.get_channel(channel_id).await? {
            self.redis
                .hdel("discord:cache:channels", channel_id.get())
                .await?;

            if let Some(guild_id) = old_channel.guild_id {
                self.redis
                    .hdel(
                        "discord:cache:guild_channels:".to_owned() + &guild_id.to_string(),
                        channel_id.get(),
                    )
                    .await?;
            }
        }

        Ok(())
    }

    // apparently this is unused upstream
    async fn _remove_user(&mut self, _user_id: Id<UserMarker>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remove_role(
        &mut self,
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
    ) -> anyhow::Result<()> {
        debug!("remove_role");
        self.redis
            .hdel("discord:cache:roles", role_id.get())
            .await?;

        self.redis
            .hdel(
                "discord:cache:guild_roles:".to_owned() + &guild_id.get().to_string(),
                role_id.get(),
            )
            .await?;

        Ok(())
    }

    // just doing cache models here for now, since they're only used inside the cache itself
    async fn get_channel(
        &mut self,
        channel_id: Id<ChannelMarker>,
    ) -> anyhow::Result<Option<proto::CachedChannel>> {
        debug!("get_channel");
        let data: Option<Vec<u8>> = self
            .redis
            .hget("discord:cache:channels", RedisKey::from(channel_id.get()))
            .await?;

        match data {
            // this is cursed
            Some(buf) => Ok(proto::CachedChannel::decode(&*buf).map(|v| Some(v))?),
            None => Ok(None),
        }
    }
}

pub async fn connect_to_redis(addr: &str) -> anyhow::Result<RedisPool> {
    let pool = RedisPool::new(
        RedisConfig {
            server: fred::types::ServerConfig::Centralized {
                host: "127.0.0.1".to_string(),
                port: 6379,
            },
            ..Default::default()
        },
        10,
    )?;

    info!("connecting to redis at {}...", addr);
    pool.connect(None);
    pool.wait_for_connect().await?;

    Ok(pool)
}
