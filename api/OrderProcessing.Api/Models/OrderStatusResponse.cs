using System.Text.Json.Serialization;

namespace OrderProcessing.Api.Models;

/// <summary>
/// Response returned by GET /api/orders/{id} — the fully processed order
/// or a 404 if the correlation_id / order_id has not been seen, or a 202
/// with status "processing" if accepted but not yet completed.
/// </summary>
public record OrderStatusResponse
{
    [JsonPropertyName("order_id")]
    public Guid OrderId { get; init; }

    [JsonPropertyName("correlation_id")]
    public Guid CorrelationId { get; init; }

    [JsonPropertyName("status")]
    public string Status { get; init; } = "accepted";

    [JsonPropertyName("customer_name")]
    public string? CustomerName { get; init; }

    [JsonPropertyName("product")]
    public string? Product { get; init; }

    [JsonPropertyName("quantity")]
    public int? Quantity { get; init; }

    [JsonPropertyName("total_amount")]
    public decimal? TotalAmount { get; init; }

    [JsonPropertyName("created_at")]
    public DateTime? CreatedAt { get; init; }

    [JsonPropertyName("processed_at")]
    public DateTime? ProcessedAt { get; init; }

    [JsonPropertyName("retry_count")]
    public int? RetryCount { get; init; }

    [JsonPropertyName("last_error")]
    public string? LastError { get; init; }
}
