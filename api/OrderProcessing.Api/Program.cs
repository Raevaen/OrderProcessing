using System.Text.Json;
using System.Text.Json.Serialization;
using OrderProcessing.Api.Messaging;
using OrderProcessing.Api.Services;

var builder = WebApplication.CreateBuilder(args);

// --- Configuration ---
builder.Configuration
    .SetBasePath(Directory.GetCurrentDirectory())
    .AddJsonFile("appsettings.json", optional: false, reloadOnChange: true)
    .AddEnvironmentVariables()
    .AddCommandLine(args);

// --- Services ---
builder.Services.AddControllers()
    .AddJsonOptions(opts =>
    {
        opts.JsonSerializerOptions.PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower;
        opts.JsonSerializerOptions.DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull;
    });

builder.Services.AddSingleton<RabbitMqConnection>();
builder.Services.AddScoped<MessagePublisher>();

builder.Services.AddHealthChecks();

var app = builder.Build();

// --- Middleware ---
app.MapControllers();
app.MapHealthChecks("/health");

app.Run();
