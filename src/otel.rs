use opentelemetry::trace::{Tracer, TracerProvider as _};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::tonic_types::metadata::MetadataMap;
use opentelemetry_otlp::{LogExporter, SpanExporter, WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{LogProcessor, SdkLoggerProvider};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn setup_otlp(
    endpoint: &str,
    service_name: &str,
) -> Result<opentelemetry_sdk::trace::Tracer, Box<dyn std::error::Error>> {
    let mut metadata = MetadataMap::new();
    metadata.insert(SERVICE_NAME, service_name.parse().unwrap());

    let span_exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_metadata(metadata.clone())
        .build()?;

    let log_exporter = LogExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_metadata(metadata)
        .build()?;

    let provider: SdkLoggerProvider = SdkLoggerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_service_name(service_name.to_string())
                .build(),
        )
        .with_batch_exporter(log_exporter)
        .build();

    let layer = OpenTelemetryTracingBridge::new(&provider);
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .build();

    let tracer = provider.tracer("ollama_code");

    // Create a tracing layer with the configured tracer
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer.clone());

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "INFO".into()),
        )
        .with(layer)
        .with(telemetry)
        .init();

    // Use the tracing subscriber `Registry`, or any other subscriber
    // that impls `LookupSpan`
    // let subscriber = Registry::default().with(telemetry);

    Ok(tracer)
}
