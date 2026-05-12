using System.ComponentModel.DataAnnotations;

namespace OrderProcessing.Api.Models;

/// <summary>
/// Client-facing request body for POST /api/orders.
/// </summary>
public record CreateOrderRequest
{
    [Required]
    [MinLength(1)]
    public string CustomerName { get; init; } = string.Empty;

    [Required]
    [MinLength(1)]
    public string Product { get; init; } = string.Empty;

    [Range(1, int.MaxValue)]
    public int Quantity { get; init; }

    [Range(0.01, double.MaxValue)]
    public decimal TotalAmount { get; init; }
}
