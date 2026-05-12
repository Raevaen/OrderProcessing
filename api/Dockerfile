# ── Build stage ──
FROM mcr.microsoft.com/dotnet/sdk:8.0 AS build
WORKDIR /src

# Restore dependencies (layer-cached).
COPY api/OrderProcessing.Api/OrderProcessing.Api.csproj .
RUN dotnet restore

# Publish the application.
COPY api/OrderProcessing.Api/ .
RUN dotnet publish -c Release -o /app --no-restore

# ── Runtime stage ──
FROM mcr.microsoft.com/dotnet/aspnet:8.0
WORKDIR /app
COPY --from=build /app .
EXPOSE 5000
ENTRYPOINT ["dotnet", "OrderProcessing.Api.dll"]
