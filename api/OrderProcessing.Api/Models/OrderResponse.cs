using System.Text.Json.Serialization;

namespace OrderProcessing.Api.Models;

/// <summary>
/// Response returned after an order is accepted and published to the broker.
/// </summary>
public record OrderResponse
{
    [JsonPropertyName("correlation_id")]
    public Guid CorrelationId { get; init; }

    [JsonPropertyName("order_id")]
    public Guid OrderId { get; init; }

    [JsonPropertyName("status")]
    public string Status { get; init; } = "accepted";
}
