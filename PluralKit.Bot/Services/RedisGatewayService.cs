using System.Text.Json;

using Serilog;

using StackExchange.Redis;

using Myriad.Gateway;
using Myriad.Serialization;

namespace PluralKit.Bot;

public class RedisGatewayService
{
    private readonly BotConfig _config;
    private readonly JsonSerializerOptions _jsonSerializerOptions;
    private ConnectionMultiplexer _redis;
    private ILogger _logger;

    public RedisGatewayService(BotConfig config, ILogger logger)
    {
        _jsonSerializerOptions = new JsonSerializerOptions().ConfigureForMyriad();
        _config = config;
        _logger = logger.ForContext<RedisGatewayService>();
    }

    private List<Task> _workers = new();

    public event Func<(int, IGatewayEvent), Task>? OnEventReceived;

    public async Task Start()
    {
        if (_redis == null)
            _redis = await ConnectionMultiplexer.ConnectAsync(_config.RedisGatewayUrl);

        foreach (var topic in _config.RedisGatewayTopics)
        {
            var redisKey = $"discord:evt:{topic}";
            _logger.Debug("Listening to redis on {redisTopic}", redisKey);
            _workers.Add(RedisLoop(redisKey));
        }
    }

    public async Task RedisLoop(string topic)
    {
        while (true)
        {
            // this is a mess
            // todo: ignore events that are "too old"
            try
            {
                var res = await _redis.GetDatabase().ExecuteAsync("blpop", new object[] { topic, 1 });
                if (res.ToString() == "(nil)") continue; // this sucks
                var message = ((RedisValue[])res)[1];
                _logger.Verbose("got event from Redis: {evt}", message);
                var packet = JsonSerializer.Deserialize<GatewayPacket>((string)message, _jsonSerializerOptions);
                var evt = DeserializeEvent(packet.EventType, (JsonElement)packet.Payload);
                if (evt == null) return;
                await OnEventReceived((packet.Sequence!.Value, evt));
            }
            catch (Exception e)
            {
                _logger.Error(e, "Error in redis loop:");
            }
        }
    }

    private IGatewayEvent? DeserializeEvent(string eventType, JsonElement payload)
    {
        if (!IGatewayEvent.EventTypes.TryGetValue(eventType, out var clrType))
        {
            _logger.Debug("Received unknown event type {EventType}", eventType);
            return null;
        }

        try
        {
            _logger.Verbose("Deserializing {EventType} to {ClrType}", eventType, clrType);
            return JsonSerializer.Deserialize(payload.GetRawText(), clrType, _jsonSerializerOptions) as IGatewayEvent;
        }
        catch (JsonException e)
        {
            _logger.Error(e, "Error deserializing event {EventType} to {ClrType}", eventType, clrType);
            return null;
        }
    }
}