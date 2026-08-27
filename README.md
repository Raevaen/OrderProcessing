# OrderProcessing

OrderProcessing is an event-driven order system built around a .NET API, a Rust processor, PostgreSQL, and RabbitMQ. The API accepts orders asynchronously and returns immediately with a `202 Accepted` response, while the processor validates and persists the order in the background.

## Prerequisites

- Docker Engine
- Docker Compose v2

## Run the system with Docker

From the project root:

```bash
docker compose up --build -d
```

This starts:

- API: http://localhost:5000
- RabbitMQ management: http://localhost:15672 (guest / guest)
- PostgreSQL: localhost:5432

Check service health:

```bash
docker compose ps
docker compose logs -f postgres rabbitmq api processor
```

Health check endpoint:

```bash
curl http://localhost:5000/health
```

## Test the API

Submit a new order:

```bash
curl -s -X POST http://localhost:5000/api/orders \
  -H "Content-Type: application/json" \
  -d '{
    "customer_name": "Alice Johnson",
    "product": "Laptop",
    "quantity": 1,
    "total_amount": 1299.99
  }'
```

Example successful response:

```json
{
  "correlation_id": "<uuid>",
  "order_id": "<uuid>",
  "status": "accepted"
}
```

Fetch the status of the order by `correlation_id` (or `order_id`):

```bash
curl -s http://localhost:5000/api/orders/<correlation_id>
```

If you want a client-controlled idempotency key:

```bash
curl -s -X POST http://localhost:5000/api/orders \
  -H "Content-Type: application/json" \
  -H "X-Correlation-Id: 123e4567-e89b-12d3-a456-426614174000" \
  -d '{
    "customer_name": "Alice Johnson",
    "product": "Laptop",
    "quantity": 1,
    "total_amount": 1299.99
  }'
```

The processor may take a moment to handle the message. Re-run the status request until the order reaches a final state such as `completed` or `failed`.

## Stop and clean up

```bash
docker compose down -v
```

This stops all containers and removes the PostgreSQL volume created for the database.
