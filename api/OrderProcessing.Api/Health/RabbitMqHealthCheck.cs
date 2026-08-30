using Microsoft.Extensions.Diagnostics.HealthChecks;
using OrderProcessing.Api.Messaging;

namespace OrderProcessing.Api.Health;

/// <summary>
/// Verifies the API can open a channel on the existing RabbitMQ connection.
/// Reuses the shared <see cref="RabbitMqConnection"/> rather than opening a
/// brand-new connection per health check.
/// </summary>
public sealed class RabbitMqHealthCheck : IHealthCheck
{
    private readonly RabbitMqConnection _connection;

    public RabbitMqHealthCheck(RabbitMqConnection connection)
    {
        _connection = connection;
    }

    public Task<HealthCheckResult> CheckHealthAsync(
        HealthCheckContext context,
        CancellationToken cancellationToken = default)
    {
        try
        {
            using var channel = _connection.CreateChannel();
            return Task.FromResult(
                channel.IsOpen
                    ? HealthCheckResult.Healthy("RabbitMQ channel opened successfully.")
                    : HealthCheckResult.Unhealthy("RabbitMQ channel could not be opened."));
        }
        catch (Exception ex)
        {
            return Task.FromResult(HealthCheckResult.Unhealthy("RabbitMQ connection failed.", ex));
        }
    }
}
