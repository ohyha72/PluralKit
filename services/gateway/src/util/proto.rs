use twilight_model::{
    channel::{
        permission_overwrite::{PermissionOverwrite, PermissionOverwriteType},
        Channel,
    },
    guild::{Guild, PartialGuild, Permissions, PremiumTier, Role},
    id::Id,
    user::User,
};

include!(concat!(env!("OUT_DIR"), "/myriad.cache.rs"));

fn overwrite_type_to_i32(r#type: PermissionOverwriteType) -> i32 {
    match r#type {
        PermissionOverwriteType::Role => 0,
        PermissionOverwriteType::Member => 1,
        _ => -1,
    }
}

fn premium_tier_to_i32(tier: PremiumTier) -> i32 {
    match tier {
        PremiumTier::None => 0,
        PremiumTier::Tier1 => 1,
        PremiumTier::Tier2 => 2,
        PremiumTier::Tier3 => 3,
        _ => -1,
    }
}

impl From<PermissionOverwrite> for Overwrite {
    fn from(item: PermissionOverwrite) -> Self {
        Overwrite {
            allow: item.allow.bits(),
            deny: item.deny.bits(),
            id: item.id.get(),
            r#type: overwrite_type_to_i32(item.kind),
        }
    }
}

impl Into<PermissionOverwrite> for Overwrite {
    fn into(self) -> PermissionOverwrite {
        PermissionOverwrite {
            allow: Permissions::from_bits_truncate(self.allow),
            deny: Permissions::from_bits_truncate(self.deny),
            id: Id::new(self.id),
            kind: PermissionOverwriteType::from(self.r#type as u8),
        }
    }
}

impl From<Channel> for CachedChannel {
    fn from(channel: Channel) -> Self {
        let overwrites = channel
            .permission_overwrites
            .map(|o| o.into_iter().map(|v| v.into()).collect());

        CachedChannel {
            guild_id: channel.guild_id.map(|id| id.get()),
            id: channel.id.get(),
            name: channel.name,
            parent_id: channel.parent_id.map(|v| v.get()),
            permission_overwrites: if let Some(o) = overwrites { o } else { vec![] },
            position: channel.position,
            r#type: u8::from(channel.kind) as _,
        }
    }
}

impl From<Guild> for CachedGuild {
    fn from(guild: Guild) -> Self {
        CachedGuild {
            id: guild.id.get(),
            name: guild.name,
            owner_id: guild.owner_id.get(),
            premium_tier: premium_tier_to_i32(guild.premium_tier),
        }
    }
}

//lol
impl From<PartialGuild> for CachedGuild {
    fn from(guild: PartialGuild) -> Self {
        CachedGuild {
            id: guild.id.get(),
            name: guild.name,
            owner_id: guild.owner_id.get(),
            premium_tier: premium_tier_to_i32(guild.premium_tier),
        }
    }
}

impl From<Role> for CachedRole {
    fn from(role: Role) -> Self {
        CachedRole {
            id: role.id.get(),
            name: role.name,
            position: role.position as i32,
            permissions: role.permissions.bits(),
            mentionable: role.mentionable,
        }
    }
}

impl From<User> for CachedUser {
    fn from(user: User) -> Self {
        CachedUser {
            // avatar: user.avatar.map(|v| {
            //     std::str::from_utf8(&v.bytes())
            //         .expect("failed to convert image hash into string")
            //         .to_string()
            // }),
            avatar: None,
            bot: user.bot,
            discriminator: user.discriminator.to_string(),
            id: user.id.get(),
            username: user.name,
        }
    }
}
