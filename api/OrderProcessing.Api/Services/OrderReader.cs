using Npgsql;
using OrderProcessing.Api.Models;

namespace OrderProcessing.Api.Services;

/// <summary>
/// Read-only access to the orders table for status queries.
/// The API never writes to the database — this is a one-way read path
/// that complements the event-driven write path.
/// </summary>
public sealed class OrderReader
{
    private readonly string _connectionString;
    private readonly ILogger<OrderReader> _logger;

    public OrderReader(IConfiguration configuration, ILogger<OrderReader> logger)
    {
        _connectionString = configuration.GetConnectionString("Postgres")
            ?? throw new InvalidOperationException("ConnectionStrings:Postgres is not configured");
        _logger = logger;
    }

    /// <summary>
    /// Look up an order by correlation_id (the idempotency key) or order_id.
    /// Falls back from the orders table to the idempotency_keys table if the
    /// order hasn't been persisted yet.
    /// </summary>
    public async Task<OrderStatusResponse?> GetOrderStatusAsync(Guid id, CancellationToken ct = default)
    {
        // 1. Try the orders table first — this is the happy path for completed orders.
        await using var conn = new NpgsqlConnection(_connectionString);
        await conn.OpenAsync(ct);

        var order = await TryGetFromOrdersAsync(conn, id, ct);
        if (order is not null)
            return order;

        // 2. Try the idempotency_keys table — the order might be in-flight.
        var idempotency = await TryGetFromIdempotencyAsync(conn, id, ct);
        if (idempotency is not null)
            return idempotency;

        return null; // 404 — never seen
    }

    private static async Task<OrderStatusResponse?> TryGetFromOrdersAsync(
        NpgsqlConnection conn,
        Guid id,
        CancellationToken ct)
    {
        const string sql = """
            SELECT
                o.id                AS order_id,
                o.correlation_id,
                o.status,
                o.customer_name,
                o.product,
                o.quantity,
                o.total_amount,
                o.created_at,
                o.processed_at,
                (SELECT MAX(attempt) FROM processing_attempts pa
                 WHERE pa.correlation_id = o.correlation_id) AS retry_count,
                (SELECT pa2.error_message FROM processing_attempts pa2
                 WHERE pa2.correlation_id = o.correlation_id
                 ORDER BY pa2.attempt DESC LIMIT 1) AS last_error
            FROM orders o
            WHERE o.correlation_id = $1 OR o.id = $1
            LIMIT 1
            """;

        await using var cmd = new NpgsqlCommand(sql, conn);
        cmd.Parameters.AddWithValue(id);

        await using var reader = await cmd.ExecuteReaderAsync(ct);
        if (!await reader.ReadAsync(ct))
            return null;

        return new OrderStatusResponse
        {
            OrderId = reader.GetGuid(0),
            CorrelationId = reader.GetGuid(1),
            Status = reader.GetString(2),
            CustomerName = reader.IsDBNull(3) ? null : reader.GetString(3),
            Product = reader.IsDBNull(4) ? null : reader.GetString(4),
            Quantity = reader.IsDBNull(5) ? null : reader.GetInt32(5),
            TotalAmount = reader.IsDBNull(6) ? null : reader.GetDecimal(6),
            CreatedAt = reader.IsDBNull(7) ? null : reader.GetDateTime(7),
            ProcessedAt = reader.IsDBNull(8) ? null : reader.GetDateTime(8),
            RetryCount = reader.IsDBNull(9) ? null : reader.GetInt32(9),
            LastError = reader.IsDBNull(10) ? null : reader.GetString(10),
        };
    }

    private static async Task<OrderStatusResponse?> TryGetFromIdempotencyAsync(
        NpgsqlConnection conn,
        Guid correlationId,
        CancellationToken ct)
    {
        const string sql = """
            SELECT
                ik.correlation_id,
                ik.status,
                ik.created_at,
                (SELECT MAX(attempt) FROM processing_attempts pa
                 WHERE pa.correlation_id = ik.correlation_id) AS retry_count,
                (SELECT pa2.error_message FROM processing_attempts pa2
                 WHERE pa2.correlation_id = ik.correlation_id
                 ORDER BY pa2.attempt DESC LIMIT 1) AS last_error
            FROM idempotency_keys ik
            WHERE ik.correlation_id = $1
            LIMIT 1
            """;

        await using var cmd = new NpgsqlCommand(sql, conn);
        cmd.Parameters.AddWithValue(correlationId);

        await using var reader = await cmd.ExecuteReaderAsync(ct);
        if (!await reader.ReadAsync(ct))
            return null;

        return new OrderStatusResponse
        {
            CorrelationId = reader.GetGuid(0),
            Status = reader.GetString(1) switch
            {
                "processing" => "accepted",
                "failed" => "failed",
                var s => s,
            },
            CreatedAt = reader.IsDBNull(2) ? null : reader.GetDateTime(2),
            RetryCount = reader.IsDBNull(3) ? null : reader.GetInt32(3),
            LastError = reader.IsDBNull(4) ? null : reader.GetString(4),
        };
    }
}
