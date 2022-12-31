using System.Text.Json;

using App.Metrics;

using Autofac;

using Myriad.Cache;
using Myriad.Gateway;
using Myriad.Serialization;
using Myriad.Types;

using NodaTime;

using StackExchange.Redis;

using PluralKit.Core;

using Serilog;

namespace GatewayWorker;

public class WorkerMainService
{
    private readonly IDiscordCache _cache;

    private readonly Cluster _cluster;
    private readonly GatewayWorkerConfig _config;
    private readonly ILogger _logger;
    private readonly IMetrics _metrics;
    private readonly RedisService _redis;
    private readonly ILifetimeScope _services;
    private ConnectionMultiplexer _evtRedis;
    private JsonSerializerOptions _jsonSerializerOptions;

    private Timer _periodicTask; // Never read, just kept here for GC reasons

    public WorkerMainService(ILifetimeScope services, ILogger logger, IMetrics metrics,
               GatewayWorkerConfig config, RedisService redis, Cluster cluster, IDiscordCache cache)
    {
        _logger = logger.ForContext<WorkerMainService>();
        _services = services;
        _metrics = metrics;
        _config = config;
        _redis = redis;
        _cluster = cluster;
        _cache = cache;

        _jsonSerializerOptions = new JsonSerializerOptions().ConfigureForMyriad();
    }

    private string BotStatus => $"{(_config.Prefixes ?? GatewayWorkerConfig.DefaultPrefixes)[0]}help"
        + (CustomStatusMessage != null ? $" | {CustomStatusMessage}" : "");
    public string CustomStatusMessage = null;

    public async Task Init()
    {
        _evtRedis = await ConnectionMultiplexer.ConnectAsync(_config.RedisGatewayUrl);

        _cluster.EventReceived += (shard, evt) => OnEventReceived(shard.ShardId, evt);
        _cluster.DiscordPresence = new GatewayStatusUpdate
        {
            Status = GatewayStatusUpdate.UserStatus.Online,
            Activities = new[]
            {
                new Activity
                {
                    Type = ActivityType.Game,
                    Name = BotStatus
                }
            }
        };

        // Init the shard stuff
        _services.Resolve<ShardInfoService>().Init();

        // Not awaited, just needs to run in the background
        // Trying our best to run it at whole minute boundaries (xx:00), with ~250ms buffer
        // This *probably* doesn't matter in practice but I jut think it's neat, y'know.
        var timeNow = SystemClock.Instance.GetCurrentInstant();
        var timeTillNextWholeMinute = TimeSpan.FromMilliseconds(60000 - timeNow.ToUnixTimeMilliseconds() % 60000 + 250);
        _periodicTask = new Timer(_ =>
        {
            var __ = UpdatePeriodic();
        }, null, timeTillNextWholeMinute, TimeSpan.FromMinutes(1));
    }

    private async Task OnEventReceived(int shardId, IGatewayEvent evt)
    {
        // we HandleGatewayEvent **before** getting the own user, because the own user is set in HandleGatewayEvent for ReadyEvent
        await _cache.HandleGatewayEvent(evt);

        await _cache.TryUpdateSelfMember(_config.ClientId, evt);

        await DispatchEvent(shardId, evt);
    }

    private Task DispatchEvent(int shardId, IGatewayEvent evt)
    {
        if (Environment.GetEnvironmentVariable("IGNORE_EVENTS") == "true")
            return Task.CompletedTask;

        if (evt is MessageCreateEvent mc)
        {
            if (mc.Author.Bot)
                return DispatchEventInner(shardId, mc, "MESSAGE_CREATE", "log_cleanup");
            else if (HasCommandPrefix(mc.Content))
                return DispatchEventInner(shardId, mc, "MESSAGE_CREATE", "command");
            else if (mc.GuildId.HasValue)
                return DispatchEventInner(shardId, mc, "MESSAGE_CREATE", "proxy");
        }
        else if (evt is MessageUpdateEvent mu && mu.GuildId.HasValue)
            return DispatchEventInner(shardId, mu, "MESSAGE_UPDATE", "proxy");
        // ignored for now, since we don't do anything with delete events right now
        // else if (evt is MessageDeleteEvent md && md.GuildId.HasValue)
        //     return DispatchEventInner(shardId, md, "MESSAGE_DELETE", "message_delete");
        // else if (evt is MessageDeleteBulkEvent mdb && mdb.GuildId.HasValue)
        //     return DispatchEventInner(shardId, mdb, "MESSAGE_DELETE_BULK", "message_delete");
        else if (evt is MessageReactionAddEvent mra)
            return DispatchEventInner(shardId, mra, "MESSAGE_REACTION_ADD", "reaction");
        else if (evt is InteractionCreateEvent ic)
            return DispatchEventInner(shardId, ic, "INTERACTION_CREATE", "interaction");

        return Task.CompletedTask;
    }

    private bool HasCommandPrefix(string message)
    {
        // First, try prefixes defined in the config
        var prefixes = _config.Prefixes ?? GatewayWorkerConfig.DefaultPrefixes;
        foreach (var prefix in prefixes)
        {
            if (!message.StartsWith(prefix, StringComparison.InvariantCultureIgnoreCase)) continue;

            return true;
        }

        // Then, check mention prefix (must be the bot user, ofc)
        int argPos = -1;
        if (PluralKit.Bot.DiscordUtils.HasMentionPrefix(message, ref argPos, out var id))
            return id == _config.ClientId;

        return false;
    }

    private async Task DispatchEventInner(int shardId, IGatewayEvent evt, string eventType, string topic)
    {
        _logger.Verbose("Dispatching {GatewayEvent} ({eventType}) to {topic}", evt, eventType, topic);

        await _evtRedis.GetDatabase().ListRightPushAsync(
            "discord:evt:" + topic,
            JsonSerializer.SerializeToUtf8Bytes(new GatewayPacket()
            {
                Sequence = shardId,
                EventType = eventType,
                Payload = evt
            }, _jsonSerializerOptions)
        );
    }

    public async Task Shutdown()
    {
        // This will stop the timer and prevent any subsequent invocations
        await _periodicTask.DisposeAsync();

        // Send users a lil status message
        // We're not actually properly disconnecting from the gateway (lol)  so it'll linger for a few minutes
        // Should be plenty of time for the bot to connect again next startup and set the real status
        await Task.WhenAll(_cluster.Shards.Values.Select(shard =>
            shard.UpdateStatus(new GatewayStatusUpdate
            {
                Activities = new[]
                {
                    new Activity
                    {
                        Name = "Restarting... (please wait)",
                        Type = ActivityType.Game
                    }
                },
                Status = GatewayStatusUpdate.UserStatus.Idle
            })));
    }

    private async Task UpdatePeriodic()
    {
        _logger.Debug("Running once-per-minute scheduled tasks");

        // Check from a new custom status from Redis and update Discord accordingly
        var newStatus = await _redis.Connection.GetDatabase().StringGetAsync("pluralkit:botstatus");
        if (newStatus != CustomStatusMessage)
        {
            CustomStatusMessage = newStatus;

            _logger.Information("Pushing new bot status message to Discord");
            await Task.WhenAll(_cluster.Shards.Values.Select(shard =>
                shard.UpdateStatus(new GatewayStatusUpdate
                {
                    Activities = new[]
                    {
                        new Activity
                        {
                            Name = BotStatus,
                            Type = ActivityType.Game
                        }
                    },
                    Status = GatewayStatusUpdate.UserStatus.Online
                })));
        }

        // Collect some stats, submit them to the metrics backend
        // todo
        // await _collector.CollectStats();
        await Task.WhenAll(((IMetricsRoot)_metrics).ReportRunner.RunAllAsync());
        _logger.Debug("Submitted metrics to backend");
    }
}