#!/usr/bin/env bash
# Creates/updates the "orderprocessing-secrets" Kubernetes Secret from the
# repo's local .env file, so no credentials ever need to be hardcoded in the
# Deployment manifests (postgres.yaml, rabbitmq.yaml, orderapi.yaml,
# orderprocessor.yaml).
#
# Run this once before `kubectl apply -f <manifest>.yaml`, and again any time
# a value in .env changes.
set -euo pipefail

cd "$(dirname "$0")/.."

if [ ! -f .env ]; then
  echo ".env not found. Copy .env.example to .env and fill in real values first." >&2
  exit 1
fi

set -a
# shellcheck disable=SC1091
source .env
set +a

: "${POSTGRES_USER:?POSTGRES_USER must be set in .env}"
: "${POSTGRES_PASSWORD:?POSTGRES_PASSWORD must be set in .env}"
: "${POSTGRES_DB:?POSTGRES_DB must be set in .env}"
: "${RABBITMQ_USERNAME:?RABBITMQ_USERNAME must be set in .env}"
: "${RABBITMQ_PASSWORD:?RABBITMQ_PASSWORD must be set in .env}"

kubectl create secret generic orderprocessing-secrets \
  --from-literal=POSTGRES_USER="${POSTGRES_USER}" \
  --from-literal=POSTGRES_PASSWORD="${POSTGRES_PASSWORD}" \
  --from-literal=POSTGRES_DB="${POSTGRES_DB}" \
  --from-literal=RABBITMQ_USERNAME="${RABBITMQ_USERNAME}" \
  --from-literal=RABBITMQ_PASSWORD="${RABBITMQ_PASSWORD}" \
  --from-literal=POSTGRES_CONNECTION_STRING="Host=postgres;Port=5432;Database=${POSTGRES_DB};Username=${POSTGRES_USER};Password=${POSTGRES_PASSWORD}" \
  --from-literal=RABBITMQ_URL="amqp://${RABBITMQ_USERNAME}:${RABBITMQ_PASSWORD}@rabbitmq:5672/%2F" \
  --from-literal=DATABASE_URL="postgresql://${POSTGRES_USER}:${POSTGRES_PASSWORD}@postgres:5432/${POSTGRES_DB}" \
  --dry-run=client -o yaml | kubectl apply -f -

echo "Secret 'orderprocessing-secrets' created/updated."
