using System.Text;
using System.Text.Json;
using OrderProcessing.Api.Messaging;
using OrderProcessing.Api.Models;
using RabbitMQ.Client;

namespace OrderProcessing.Api.Services;

/// <summary>
/// Publishes <see cref="Order"/> messages to the topic exchange.
/// Sets message_id, correlation_id, and persistent delivery mode.
/// </summary>
public sealed class MessagePublisher
{
    private readonly RabbitMqConnection _connection;
    private readonly ILogger<MessagePublisher> _logger;

    public MessagePublisher(RabbitMqConnection connection, ILogger<MessagePublisher> logger)
    {
        _connection = connection;
        _logger = logger;
    }

    public void Publish(Order order)
    {
        using var channel = _connection.CreateChannel();

        var body = JsonSerializer.SerializeToUtf8Bytes(order);

        var properties = channel.CreateBasicProperties();
        properties.MessageId = order.OrderId.ToString();
        properties.CorrelationId = order.CorrelationId.ToString();
        properties.Persistent = true;
        properties.ContentType = "application/json";
        properties.Timestamp = new AmqpTimestamp(DateTimeOffset.UtcNow.ToUnixTimeSeconds());

        _logger.LogInformation(
            "Publishing order {OrderId} correlation {CorrelationId} to exchange={Exchange} routing_key={RoutingKey}",
            order.OrderId,
            order.CorrelationId,
            _connection.Exchange,
            _connection.RoutingKey);

        channel.BasicPublish(
            exchange: _connection.Exchange,
            routingKey: _connection.RoutingKey,
            mandatory: true,
            basicProperties: properties,
            body: body);

        _logger.LogInformation(
            "Published order {OrderId} with correlation {CorrelationId}",
            order.OrderId, order.CorrelationId);
    }
}
