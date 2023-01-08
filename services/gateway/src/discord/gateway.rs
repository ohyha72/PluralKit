use futures::stream::StreamExt;
use std::sync::{
    mpsc::{channel, Receiver},
    Arc,
};
use tracing::{info, warn};
use twilight_gateway::{cluster::ShardScheme, Cluster, Event};
use twilight_model::gateway::payload::outgoing::update_presence::UpdatePresencePayload;
use twilight_model::gateway::{
    presence::{Activity, ActivityType, Status},
    Intents,
};

use crate::config::config;
use crate::discord::identify_queue;

pub async fn connect() -> anyhow::Result<(Arc<Cluster>, Receiver<(u64, Event)>)> {
    let (tx, rx) = channel();

    let config = config().await?;

    let intents = Intents::GUILDS
        | Intents::DIRECT_MESSAGES
        | Intents::DIRECT_MESSAGE_REACTIONS
        | Intents::GUILD_MESSAGES
        | Intents::GUILD_MESSAGE_REACTIONS
        | Intents::MESSAGE_CONTENT;

    let queue = Arc::new(identify_queue::init_gateway_queue().await?);

    let (start_shard, end_shard) = if config.total_shards < 16 {
        warn!("we have less than 16 shards, assuming single gateway process");
        (0, config.total_shards - 1)
    } else {
        ((config.node_id * 16), ((config.node_id + 1) * 16) - 1)
        // (1u16, 1u16)
    };

    info!("aaa {} {}", start_shard, end_shard);

    let (cluster, mut events) = Cluster::builder(config.token.to_owned(), intents)
        .shard_scheme(ShardScheme::Range {
            from: start_shard.into(),
            to: end_shard.into(),
            total: config.total_shards.into(),
        })
        .presence(presence("pk;help", false))
        .queue(queue)
        .build()
        .await?;

    let cluster = Arc::new(cluster);
    let cluster_spawn = Arc::clone(&cluster);

    // spawn event listener
    tokio::spawn(async move {
        while let Some((shard_id, event)) = events.next().await {
            if let Err(err) = tx.send((shard_id, event)) {
                tracing::error!("error sending event: {:#?}", err);
            }
        }
    });

    cluster_spawn.up().await;

    Ok((cluster, rx))
}

pub fn presence(status: &str, going_away: bool) -> UpdatePresencePayload {
    UpdatePresencePayload {
        activities: vec![Activity {
            application_id: None,
            assets: None,
            buttons: vec![],
            created_at: None,
            details: None,
            id: None,
            state: None,
            url: None,
            emoji: None,
            flags: None,
            instance: None,
            kind: ActivityType::Playing,
            name: status.to_string(),
            party: None,
            secrets: None,
            timestamps: None,
        }],
        afk: false,
        since: None,
        status: if going_away {
            Status::Idle
        } else {
            Status::Online
        },
    }
}
