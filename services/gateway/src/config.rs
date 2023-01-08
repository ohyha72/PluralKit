use tokio::sync::OnceCell;

pub struct Config {
    pub client_id: u64,
    pub token: String,
    pub concurrency: u64,
    pub redis_addr: String,
    pub gateway_redis_addr: String,
    pub node_id: u16,
    pub total_nodes: u16,
    pub total_shards: u16,
    pub ignore_events: bool,
}

static CONFIG: OnceCell<Config> = OnceCell::const_new();

pub async fn config() -> anyhow::Result<&'static Config> {
    CONFIG.get_or_try_init(config_inner).await
}

async fn config_inner() -> anyhow::Result<Config> {
    // todo: get from file / environment
    Ok(Config {
        // client_id: 865783137254506526,
        // token: "<snip>".to_string(),
        // concurrency: 1,
        // total_nodes: 1,
        // total_shards: 1,

        // prod
        client_id: 466378653216014359,
        token: "<snip>".to_string(),
        concurrency: 16,
        total_nodes: 32,
        total_shards: 512,

        redis_addr: "redis://127.0.0.1:6379".to_string(),
        gateway_redis_addr: "redis://127.0.0.1:6379".to_string(),
        node_id: 1,
        ignore_events: true,
    })
}
