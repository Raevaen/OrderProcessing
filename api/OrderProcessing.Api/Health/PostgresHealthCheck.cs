using Microsoft.Extensions.Diagnostics.HealthChecks;
using Npgsql;

namespace OrderProcessing.Api.Health;

/// <summary>
/// Verifies the API can open a connection to Postgres and run a trivial query.
/// </summary>
public sealed class PostgresHealthCheck : IHealthCheck
{
    private readonly string _connectionString;

    public PostgresHealthCheck(IConfiguration configuration)
    {
        _connectionString = configuration.GetConnectionString("Postgres")
            ?? throw new InvalidOperationException("ConnectionStrings:Postgres is not configured");
    }

    public async Task<HealthCheckResult> CheckHealthAsync(
        HealthCheckContext context,
        CancellationToken cancellationToken = default)
    {
        try
        {
            await using var conn = new NpgsqlConnection(_connectionString);
            await conn.OpenAsync(cancellationToken);

            await using var cmd = new NpgsqlCommand("SELECT 1", conn);
            await cmd.ExecuteScalarAsync(cancellationToken);

            return HealthCheckResult.Healthy("Postgres connection succeeded.");
        }
        catch (Exception ex)
        {
            return HealthCheckResult.Unhealthy("Postgres connection failed.", ex);
        }
    }
}
