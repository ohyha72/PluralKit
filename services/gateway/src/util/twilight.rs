use twilight_model::{
    channel::message::Mention,
    gateway::payload::incoming::MemberUpdate,
    guild::{Member, PartialMember},
    user::User,
};

// i'm sure there's a better way to do this, but it works for now

pub fn mention_to_user(item: Mention) -> User {
    User {
        accent_color: None,
        avatar: item.avatar,
        banner: None,
        bot: item.bot,
        discriminator: item.discriminator,
        email: None,
        flags: None,
        id: item.id,
        locale: None,
        mfa_enabled: None,
        name: item.name,
        premium_type: None,
        public_flags: Some(item.public_flags),
        system: None,
        verified: None,
    }
}

pub fn member_to_partial_member(item: Member) -> PartialMember {
    PartialMember {
        avatar: item.avatar,
        communication_disabled_until: item.communication_disabled_until,
        deaf: item.deaf,
        joined_at: item.joined_at,
        mute: item.mute,
        nick: item.nick,
        permissions: None, // only sent in interaction
        premium_since: item.premium_since,
        roles: item.roles,
        user: Some(item.user),
    }
}

pub fn member_update_to_partial_member(item: MemberUpdate) -> PartialMember {
    PartialMember {
        avatar: item.avatar,
        communication_disabled_until: item.communication_disabled_until,
        deaf: false, // is Option in MemberUpdate
        joined_at: item.joined_at,
        mute: false, // is Option in MemberUpdate,
        nick: item.nick,
        permissions: None, // only sent in interaction
        premium_since: item.premium_since,
        roles: item.roles,
        user: Some(item.user),
    }
}
