using Microsoft.AspNetCore.Mvc;
using OrderProcessing.Api.Models;
using OrderProcessing.Api.Services;

namespace OrderProcessing.Api.Controllers;

/// <summary>
/// Accepts order submissions and publishes them as events to the broker.
/// </summary>
[ApiController]
[Route("api/[controller]")]
public class OrdersController : ControllerBase
{
    private readonly MessagePublisher _publisher;
    private readonly ILogger<OrdersController> _logger;

    public OrdersController(MessagePublisher publisher, ILogger<OrdersController> logger)
    {
        _publisher = publisher;
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

        // Respect client-supplied idempotency key, else generate one.
        var correlationId = TryExtractCorrelationId() ?? Guid.NewGuid();
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

        return AcceptedAtAction(nameof(CreateOrder), null, response);
    }

    /// <summary>
    /// Reads the <c>X-Correlation-Id</c> header for client-supplied idempotency.
    /// </summary>
    private Guid? TryExtractCorrelationId()
    {
        var header = Request.Headers["X-Correlation-Id"].FirstOrDefault();
        return Guid.TryParse(header, out var id) ? id : null;
    }
}
