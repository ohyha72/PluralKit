use redis::aio::ConnectionManager;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{error, info};
use twilight_gateway::queue::Queue;

use crate::config;

#[derive(Clone)]
pub struct RedisQueue {
    pub redis: ConnectionManager,
    pub concurrency: u64,
}

impl Debug for RedisQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisQueue")
            .field("concurrency", &self.concurrency)
            .finish()
    }
}

impl Queue for RedisQueue {
    fn request<'a>(
        &'a self,
        shard_id: [u64; 2],
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = ()> + Send + 'a>> {
        Box::pin(request_inner(
            self.redis.clone(),
            self.concurrency,
            *shard_id.first().unwrap(),
        ))
    }
}

const EXPIRY: i8 = 6;
const RETRY_INTERVAL: u64 = 500;

async fn request_inner(mut client: ConnectionManager, concurrency: u64, shard_id: u64) {
    let bucket = shard_id % concurrency;
    let key = format!("pluralkit:identify:{}", bucket);

    // SET bucket 1 EX 6 NX = write a key expiring after 6 seconds if there's not already one
    let mut cmd = redis::cmd("SET");
    cmd.arg(key).arg("1").arg("EX").arg(EXPIRY).arg("NX");

    info!(shard_id, bucket, "waiting for allowance...");
    loop {
        let done = cmd
            .clone()
            .query_async::<_, Option<String>>(&mut client)
            .await;
        match done {
            Ok(Some(_)) => {
                info!(shard_id, bucket, "got allowance!");
                return;
            }
            Ok(None) => {
                // not allowed yet, waiting
            }
            Err(e) => {
                error!(shard_id, bucket, "error getting shard allowance: {}", e)
            }
        }

        tokio::time::sleep(Duration::from_millis(RETRY_INTERVAL)).await;
    }
}

pub async fn connect_to_redis(addr: &str) -> anyhow::Result<ConnectionManager> {
    let client = redis::Client::open(addr)?;
    info!("connecting to redis at {}...", addr);
    Ok(ConnectionManager::new(client).await?)
}

pub async fn init_gateway_queue() -> anyhow::Result<RedisQueue> {
    let cfg = config::config().await?;
    let redis = connect_to_redis(&cfg.redis_addr).await?;
    let concurrency = cfg.concurrency;
    Ok(RedisQueue { redis, concurrency })
}
