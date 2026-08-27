using RabbitMQ.Client;
using RabbitMQ.Client.Exceptions;

namespace OrderProcessing.Api.Messaging;

/// <summary>
/// Singleton wrapper around a RabbitMQ <see cref="IConnection"/>.
/// Declares the topology (exchange + dead-letter) on startup so the publisher
/// never hits a missing-exchange error.
/// </summary>
public sealed class RabbitMqConnection : IDisposable
{
    private readonly IConnection _connection;
    private readonly IModel _declareChannel;
    private readonly ILogger<RabbitMqConnection> _logger;

    public string HostName { get; }
    public int Port { get; }
    public string VirtualHost { get; }

    public string Exchange { get; }
    public string RoutingKey { get; }

    public RabbitMqConnection(
        IConfiguration configuration,
        ILogger<RabbitMqConnection> logger)
    {
        _logger = logger;
        var section = configuration.GetSection("RabbitMq");
        HostName = section["HostName"] ?? "rabbitmq";
        Port = int.Parse(section["Port"] ?? "5672");
        VirtualHost = section["VirtualHost"] ?? "/";

        var factory = new ConnectionFactory
        {
            HostName = HostName,
            Port = Port,
            UserName = section["UserName"] ?? "guest",
            Password = section["Password"] ?? "guest",
            VirtualHost = VirtualHost,
            AutomaticRecoveryEnabled = true,
            NetworkRecoveryInterval = TimeSpan.FromSeconds(5),
            RequestedHeartbeat = TimeSpan.FromSeconds(30)
        };

        Exchange = section["Exchange"] ?? "orders.exchange";
        RoutingKey = section["RoutingKey"] ?? "order.created";

        _connection = factory.CreateConnection();
        _declareChannel = _connection.CreateModel();

        DeclareTopology();

        _logger.LogInformation(
            "RabbitMQ connection ready host={Host} port={Port} vhost={VirtualHost} exchange={Exchange} routing_key={RoutingKey}",
            HostName,
            Port,
            VirtualHost,
            Exchange,
            RoutingKey);
    }

    /// <summary>
    /// Declare the primary exchange, dead-letter exchange, primary queue,
    /// and dead-letter queue. All declarations are idempotent.
    /// </summary>
    private void DeclareTopology()
    {
        // Dead-letter exchange + queue
        _declareChannel.ExchangeDeclare("orders.dlx", ExchangeType.Topic, durable: true, autoDelete: false);
        _declareChannel.QueueDeclare("orders.created.dlq", durable: true, exclusive: false, autoDelete: false);
        _declareChannel.QueueBind("orders.created.dlq", "orders.dlx", "order.created");

        // Arguments that route failed messages to the DLX after rejection
        var args = new Dictionary<string, object>
        {
            { "x-dead-letter-exchange", "orders.dlx" },
            { "x-dead-letter-routing-key", "order.created" }
        };

        // Primary queue bound to the exchange
        _declareChannel.ExchangeDeclare(Exchange, ExchangeType.Topic, durable: true, autoDelete: false);
        _declareChannel.QueueDeclare("orders.created.queue", durable: true, exclusive: false, autoDelete: false, args);
        _declareChannel.QueueBind("orders.created.queue", Exchange, RoutingKey);

        _logger.LogInformation(
            "RabbitMQ topology declared queue={Queue} exchange={Exchange} routing_key={RoutingKey} dlq={DeadLetterQueue}",
            "orders.created.queue",
            Exchange,
            RoutingKey,
            "orders.created.dlq");
    }

    /// <summary>
    /// Returns a fresh <see cref="IModel"/> channel. Callers must dispose it.
    /// </summary>
    public IModel CreateChannel() => _connection.CreateModel();

    public void Dispose()
    {
        _declareChannel?.Dispose();
        _connection?.Dispose();
    }
}
