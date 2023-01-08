use chrono::Timelike;
use redis::{aio::ConnectionManager, AsyncCommands};
use serde::Serialize;
use std::time::Duration;
use tracing::{debug, error, info};
use twilight_gateway::Event;
use twilight_model::gateway::{event::DispatchEvent, payload::outgoing::UpdatePresence};

mod config;
mod discord;
mod util;

use signal_hook::{consts::SIGINT, iterator::Signals};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let mut shard_state = discord::shard_state::init().await?;
    let mut redis_cache = discord::cache::init().await?;

    let config = config::config().await?;
    let rconn = connect_to_redis(&config.gateway_redis_addr).await?;

    let (cluster, rx) = discord::gateway::connect().await?;

    let mut signals = Signals::new(&[SIGINT])?;

    tokio::spawn(async move {
        for sig in signals.forever() {
            // todo: set restarting presence
            println!("Received signal {:?}", sig);
        }
    });

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(
                (60 - chrono::offset::Utc::now().second()).into(),
            ))
            .await;
            info!("running per-minute scheduled tasks");

            // todo: fetch presence from redis and only update if it changed

            let presence = UpdatePresence {
                op: twilight_model::gateway::OpCode::PresenceUpdate,
                d: discord::gateway::presence("pk;help", false),
            };

            for shard in cluster.shards() {
                if let Err(err) = shard.command(&presence).await {
                    error!(
                        "error updating presence on shard {}: {:?}",
                        shard.config().shard()[0],
                        err
                    );
                }
            }
        }
    });

    loop {
        if let Err(err) = {
            let (shard_id, event) = rx.recv()?;
            shard_state.handle_event(shard_id, event.clone()).await?;
            redis_cache.handle_event(event.clone()).await?;
            // dispatch_event(shard_id, event.clone(), rconn.clone()).await?;
            test_handle_event(shard_id, event).await?;
            Ok::<(), anyhow::Error>(())
        } {
            error!("processing event: {:#?}", err);
        }
    }
}

async fn test_handle_event(shard_id: u64, event: Event) -> anyhow::Result<()> {
    match event {
        Event::MessageCreate(msg) if msg.content == "test" => {
            println!("got message");
        }
        _ => {}
    }

    Ok(())
}

async fn dispatch_event(
    shard_id: u64,
    event: Event,
    rconn: ConnectionManager,
) -> anyhow::Result<()> {
    match event {
        Event::MessageCreate(_) => {
            dispatch_inner(shard_id, event, "MESSAGE_CREATE", "command", rconn).await
        }
        _ => Ok(()),
    }
}

async fn dispatch_inner(
    shard_id: u64,
    event: Event,
    event_type: &str,
    topic: &str,
    mut rconn: ConnectionManager,
) -> anyhow::Result<()> {
    debug!("dispatching event {:#?}", event);

    let event = GatewayEvent {
        op: 0,
        d: DispatchEvent::try_from(event)?,
        s: shard_id,
        t: event_type.to_owned(),
    };

    rconn
        .rpush(
            "discord:evt:".to_owned() + &topic,
            serde_json::to_vec(&event)?,
        )
        .await?;

    Ok(())
}

pub async fn connect_to_redis(addr: &str) -> anyhow::Result<ConnectionManager> {
    let client = redis::Client::open(addr)?;
    info!("connecting to redis at {}...", addr);
    Ok(ConnectionManager::new(client).await?)
}

#[derive(Serialize)]
struct GatewayEvent {
    op: u64,
    d: DispatchEvent,
    s: u64,
    t: String,
}
