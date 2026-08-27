using Microsoft.AspNetCore.Mvc;
using OrderProcessing.Api.Models;
using OrderProcessing.Api.Services;

namespace OrderProcessing.Api.Controllers;

/// <summary>
/// Accepts order submissions and publishes them as events to the broker,
/// and allows clients to check the status of previously submitted orders.
/// </summary>
[ApiController]
[Route("api/[controller]")]
public class OrdersController : ControllerBase
{
    private readonly MessagePublisher _publisher;
    private readonly OrderReader _reader;
    private readonly ILogger<OrdersController> _logger;

    public OrdersController(
        MessagePublisher publisher,
        OrderReader reader,
        ILogger<OrdersController> logger)
    {
        _publisher = publisher;
        _reader = reader;
        _logger = logger;
    }

    /// <summary>
    /// POST /api/orders — submit a new order.
    /// </summary>
    [HttpPost]
    [ProducesResponseType(typeof(OrderResponse), StatusCodes.Status202Accepted)]
    [ProducesResponseType(StatusCodes.Status400BadRequest)]
    public IActionResult CreateOrder([FromBody] CreateOrderRequest request)
    {
        if (!ModelState.IsValid)
            return BadRequest(ModelState);

        var hasClientCorrelationId = TryExtractCorrelationId(out var correlationId);
        if (!hasClientCorrelationId)
            correlationId = Guid.NewGuid();

        var orderId = Guid.NewGuid();

        var order = new Order
        {
            CorrelationId = correlationId,
            OrderId = orderId,
            CustomerName = request.CustomerName,
            Product = request.Product,
            Quantity = request.Quantity,
            TotalAmount = request.TotalAmount,
            Timestamp = DateTime.UtcNow
        };

        _logger.LogInformation(
            "Submitting order {OrderId} with correlation {CorrelationId} (source={CorrelationSource})",
            orderId,
            correlationId,
            hasClientCorrelationId ? "client-header" : "generated");

        _publisher.Publish(order);

        _logger.LogInformation(
            "Accepted order {OrderId} correlation {CorrelationId}",
            orderId, correlationId);

        var response = new OrderResponse
        {
            CorrelationId = correlationId,
            OrderId = orderId,
            Status = "accepted"
        };

        return AcceptedAtAction(nameof(GetOrderStatus), new { id = correlationId }, response);
    }

    /// <summary>
    /// GET /api/orders/{id} — check the status of a previously submitted order.
    /// Accepts either the <c>correlation_id</c> or the <c>order_id</c>.
    /// </summary>
    /// <remarks>
    /// Status values:
    ///   - "accepted"   — published to broker, not yet processed
    ///   - "processing" — idempotency claimed, being processed
    ///   - "completed"  — persisted successfully
    ///   - "failed"     — exhausted retries, sent to DLQ
    ///
    /// Returns 404 if the id is unknown.
    /// </remarks>
    [HttpGet("{id:guid}")]
    [ProducesResponseType(typeof(OrderStatusResponse), StatusCodes.Status200OK)]
    [ProducesResponseType(StatusCodes.Status404NotFound)]
    public async Task<IActionResult> GetOrderStatus([FromRoute] Guid id, CancellationToken ct)
    {
        var result = await _reader.GetOrderStatusAsync(id, ct);

        if (result is null)
            return NotFound(new { error = "Order not found", id });

        return Ok(result);
    }

    /// <summary>
    /// Reads the <c>X-Correlation-Id</c> header for client-supplied idempotency.
    /// </summary>
    private bool TryExtractCorrelationId(out Guid correlationId)
    {
        var header = Request.Headers["X-Correlation-Id"].FirstOrDefault();

        if (string.IsNullOrWhiteSpace(header))
        {
            correlationId = Guid.Empty;
            return false;
        }

        if (Guid.TryParse(header, out correlationId))
            return true;

        _logger.LogWarning(
            "Ignoring invalid X-Correlation-Id header value: {CorrelationHeader}",
            header);

        correlationId = Guid.Empty;
        return false;
    }
}
