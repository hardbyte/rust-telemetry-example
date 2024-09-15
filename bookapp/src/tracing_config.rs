use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

pub fn init_tracing() {
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    // Uses OTEL_EXPORTER_OTLP_TRACES_ENDPOINT
    // Assumes a GRPC endpoint (e.g port 4317)
    let exporter = opentelemetry_otlp::new_exporter().tonic();

    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default()
                .with_resource(opentelemetry_sdk::Resource::default()),
        )
        // a batch exporter is recommended as the simple exporter will export each span synchronously on dropping
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .expect("Failed to create tracer provider");

    // Explicitly set the tracer provider globally. Note this is now required
    // https://github.com/open-telemetry/opentelemetry-rust/blob/main/opentelemetry-otlp/CHANGELOG.md#v0170
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    // Filter the tracing layer - we can add custom filters that only impact the tracing layer
    let tracing_level_filter = tracing_subscriber::filter::Targets::new()
        .with_target("bookapp", tracing::Level::TRACE)
        .with_target("backend", tracing::Level::TRACE)
        .with_target("sqlx", tracing::Level::DEBUG)
        .with_target("tower_http", tracing::Level::INFO)
        .with_target("hyper_util", tracing::Level::INFO)
        .with_target("h2", tracing::Level::WARN)
        // Note an optional feature flag crate sets this most important trace from tracing to info level
        .with_target("otel::tracing", tracing::Level::INFO)
        .with_default(tracing::Level::INFO);

    // turn our OTLP pipeline into a tracing layer
    let tracing_opentelemetry_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer_provider.tracer("bookapp"))
        .with_filter(tracing_level_filter);

    // Configure the stdout fmt layer
    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact();

    let fmt_layer = tracing_subscriber::fmt::layer().event_format(format);

    // https://github.com/grafana/docker-otel-lgtm/issues/77
    // // Configure the Loki exporter using the builder pattern
    // let loki_url = Url::parse("http://telemetry:3100/api/v1/push").unwrap();
    //
    // let (loki_layer, loki_exporter_task) = tracing_loki::builder()
    //     .label("host", hostname::get().unwrap().into_string().unwrap()).unwrap()
    //     .extra_field("pid", std::process::id().to_string()).unwrap()
    //     .build_url(loki_url)
    //     .expect("Failed to create Loki layer");
    //
    // // Spawn the Loki background task to send logs
    // // Ensure that a Tokio runtime is running; otherwise, this will panic
    // tokio::spawn(loki_exporter_task);
    //

    // Build the subscriber by combining layers
    let subscriber = tracing_subscriber::Registry::default()
        .with(fmt_layer.with_filter(tracing_subscriber::EnvFilter::from_default_env()))
        //.with(loki_layer)
        .with(tracing_opentelemetry_layer);

    // Set the subscriber as the global default
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");
}
