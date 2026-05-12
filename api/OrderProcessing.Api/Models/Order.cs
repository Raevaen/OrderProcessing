using System.Text.Json.Serialization;

namespace OrderProcessing.Api.Models;

/// <summary>
/// Canonical order record — matches the message contract the Rust processor expects.
/// </summary>
public record Order
{
    [JsonPropertyName("correlation_id")]
    public Guid CorrelationId { get; init; }

    [JsonPropertyName("order_id")]
    public Guid OrderId { get; init; } = Guid.NewGuid();

    [JsonPropertyName("customer_name")]
    public string CustomerName { get; init; } = string.Empty;

    [JsonPropertyName("product")]
    public string Product { get; init; } = string.Empty;

    [JsonPropertyName("quantity")]
    public int Quantity { get; init; }

    [JsonPropertyName("total_amount")]
    public decimal TotalAmount { get; init; }

    [JsonPropertyName("timestamp")]
    public DateTime Timestamp { get; init; } = DateTime.UtcNow;
}
