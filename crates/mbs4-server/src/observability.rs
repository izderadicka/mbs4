use std::{
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use axum::{extract::MatchedPath, http::Request, response::Response};
use futures::future::BoxFuture;
use opentelemetry::{
    metrics::{Counter, Histogram, MeterProvider as _},
    KeyValue,
};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{Encoder, Registry, TextEncoder};
use tower::{Layer, Service};

#[derive(Clone)]
pub struct HttpMetrics {
    registry: Registry,
    _meter_provider: Arc<SdkMeterProvider>,
    request_counter: Counter<u64>,
    request_duration: Histogram<f64>,
}

impl HttpMetrics {
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();
        let exporter = opentelemetry_prometheus::exporter()
            .with_registry(registry.clone())
            .build()?;
        let meter_provider = Arc::new(SdkMeterProvider::builder().with_reader(exporter).build());
        let meter = meter_provider.meter("mbs4-server");
        let request_counter = meter
            .u64_counter("mbs4_http_requests")
            .with_description("Number of HTTP requests handled by the server")
            .build();
        let request_duration = meter
            .f64_histogram("mbs4_http_request_duration_seconds")
            .with_unit("s")
            .with_description("HTTP request duration in seconds")
            .build();

        Ok(Self {
            registry,
            _meter_provider: meter_provider,
            request_counter,
            request_duration,
        })
    }

    pub fn render_prometheus(&self) -> anyhow::Result<String> {
        let metric_families = self.registry.gather();
        let mut encoded = Vec::new();
        TextEncoder::new().encode(&metric_families, &mut encoded)?;
        Ok(String::from_utf8(encoded)?)
    }

    fn record_request(&self, method: &str, route: &str, status: u16, duration: Duration) {
        let attrs = [
            KeyValue::new("http_method", method.to_owned()),
            KeyValue::new("http_route", route.to_owned()),
            KeyValue::new("http_status_code", status.to_string()),
        ];

        self.request_counter.add(1, &attrs);
        self.request_duration.record(duration.as_secs_f64(), &attrs);
    }
}

#[derive(Clone)]
pub struct HttpMetricsLayer {
    metrics: Arc<HttpMetrics>,
}

impl HttpMetricsLayer {
    pub fn new(metrics: Arc<HttpMetrics>) -> Self {
        Self { metrics }
    }
}

impl<S> Layer<S> for HttpMetricsLayer {
    type Service = HttpMetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpMetricsService {
            inner,
            metrics: self.metrics.clone(),
        }
    }
}

#[derive(Clone)]
pub struct HttpMetricsService<S> {
    inner: S,
    metrics: Arc<HttpMetrics>,
}

impl<S, B> Service<Request<B>> for HttpMetricsService<S>
where
    S: Service<Request<B>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    B: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let started_at = Instant::now();
        let method = request.method().as_str().to_owned();
        let route = request
            .extensions()
            .get::<MatchedPath>()
            .map(MatchedPath::as_str)
            .unwrap_or_else(|| request.uri().path())
            .to_owned();
        let metrics = self.metrics.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let response = inner.call(request).await?;

            if route != "/metrics" {
                metrics.record_request(
                    &method,
                    &route,
                    response.status().as_u16(),
                    started_at.elapsed(),
                );
            }

            Ok(response)
        })
    }
}
