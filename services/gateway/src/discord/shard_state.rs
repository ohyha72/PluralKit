use prost::Message;
use redis::{aio::ConnectionManager, AsyncCommands};
use tracing::info;
use twilight_gateway::Event;

include!(concat!(env!("OUT_DIR"), "/_.rs"));

use crate::config;

#[derive(Clone)]
pub struct ShardStateManager {
    redis: ConnectionManager,
}

pub async fn init() -> anyhow::Result<ShardStateManager> {
    let cfg = config::config().await?;
    let redis = connect_to_redis(&cfg.redis_addr).await?;
    Ok(ShardStateManager { redis })
}

impl ShardStateManager {
    pub async fn handle_event(&mut self, shard_id: u64, event: Event) -> anyhow::Result<()> {
        match event {
            Event::ShardConnected(_) => self.ready_or_resumed(shard_id).await,
            Event::Resumed => self.ready_or_resumed(shard_id).await,
            Event::ShardDisconnected(_) => self.socket_closed(shard_id).await,
            Event::GatewayHeartbeat(_) => self.heartbeated(shard_id).await,
            _ => Ok(()),
        }
    }

    async fn get_shard(&mut self, shard_id: u64) -> anyhow::Result<ShardState> {
        let data = self
            .redis
            .hget::<&'static str, u64, Option<Vec<u8>>>("pluralkit:shardstatus", shard_id)
            .await?;
        match data {
            Some(buf) => {
                Ok(ShardState::decode(buf.as_slice()).expect("could not decode shard data!"))
            }
            None => Ok(ShardState::default()),
        }
    }

    async fn save_shard(&mut self, shard_id: u64, info: ShardState) -> anyhow::Result<()> {
        self.redis
            .hset::<&'static str, u64, Vec<u8>, i32>(
                "pluralkit:shardstatus",
                shard_id,
                info.encode_to_vec(),
            )
            .await?;
        Ok(())
    }

    async fn ready_or_resumed(&mut self, shard_id: u64) -> anyhow::Result<()> {
        info!("shard {} ready", shard_id);
        let mut info = self.get_shard(shard_id).await?;
        info.last_connection = chrono::offset::Utc::now().timestamp() as i32;
        info.up = true;
        self.save_shard(shard_id, info).await?;
        Ok(())
    }

    async fn socket_closed(&mut self, shard_id: u64) -> anyhow::Result<()> {
        info!("shard {} closed", shard_id);
        let mut info = self.get_shard(shard_id).await?;
        info.up = false;
        info.disconnection_count += 1;
        self.save_shard(shard_id, info).await?;
        Ok(())
    }

    async fn heartbeated(&mut self, shard_id: u64) -> anyhow::Result<()> {
        let mut info = self.get_shard(shard_id).await?;
        info.up = true;
        info.last_heartbeat = chrono::offset::Utc::now().timestamp() as i32;
        // todo
        // info.latency = latency.recent().front().map_or_else(|| 0, |d| d.as_millis()) as i32;
        info.latency = 1;
        self.save_shard(shard_id, info).await?;
        Ok(())
    }
}

pub async fn connect_to_redis(addr: &str) -> anyhow::Result<ConnectionManager> {
    let client = redis::Client::open(addr)?;
    info!("connecting to redis at {}...", addr);
    Ok(ConnectionManager::new(client).await?)
}
