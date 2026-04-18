pub use imp::Observability;

#[cfg(feature = "observability")]
mod imp {
    use std::{
        sync::Arc,
        task::{Context, Poll},
        time::{Duration, Instant},
    };

    use crate::config::ServerConfig;
    use axum::{
        extract::MatchedPath,
        http::{HeaderMap, Request, StatusCode},
        response::{IntoResponse, Response},
        routing::get,
        Router,
    };
    use futures::future::BoxFuture;
    use opentelemetry::{
        metrics::{Histogram, MeterProvider as _},
        KeyValue,
    };
    use opentelemetry_sdk::metrics::SdkMeterProvider;
    use prometheus::{Encoder, Registry, TextEncoder};
    use tower::{Layer, Service};

    const HTTP_DURATION_BUCKETS_SECONDS: [f64; 16] = [
        0.0, 0.0005, 0.001, 0.002, 0.003, 0.004, 0.005, 0.0075, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5,
        1.0, 2.0,
    ];

    #[derive(Clone)]
    pub struct Observability {
        state: Arc<ObservabilityInner>,
    }

    struct ObservabilityInner {
        http_metrics: HttpMetrics,
        metrics_token: Option<Arc<str>>,
    }

    impl Observability {
        pub fn new(config: &ServerConfig) -> anyhow::Result<Self> {
            Ok(Self {
                state: Arc::new(ObservabilityInner {
                    http_metrics: HttpMetrics::new()?,
                    metrics_token: config.metrics_token.as_deref().map(Arc::<str>::from),
                }),
            })
        }

        pub fn apply(&self, router: Router<()>) -> Router<()> {
            let mut router = router.layer(HttpMetricsLayer::new(self.clone()));

            if let Some(metrics_token) = self.state.metrics_token.clone() {
                let observability = self.clone();
                router = router.route(
                    "/metrics",
                    get(move |headers| {
                        metrics_endpoint(headers, metrics_token.clone(), observability.clone())
                    }),
                );
            }

            router
        }

        fn record_http_request(
            &self,
            method: &str,
            route: Option<&str>,
            status: u16,
            duration: Duration,
        ) {
            self.state
                .http_metrics
                .record_request(method, route, status, duration);
        }

        fn render_prometheus(&self) -> anyhow::Result<String> {
            self.state.http_metrics.render_prometheus()
        }
    }

    #[derive(Clone)]
    struct HttpMetrics {
        registry: Registry,
        _meter_provider: Arc<SdkMeterProvider>,
        request_duration: Histogram<f64>,
    }

    impl HttpMetrics {
        fn new() -> anyhow::Result<Self> {
            let registry = Registry::new();
            let exporter = opentelemetry_prometheus::exporter()
                .with_registry(registry.clone())
                .build()?;
            let meter_provider =
                Arc::new(SdkMeterProvider::builder().with_reader(exporter).build());
            let meter = meter_provider.meter("mbs4-server");
            let request_duration = meter
                .f64_histogram("http.server.request.duration")
                .with_unit("s")
                .with_boundaries(HTTP_DURATION_BUCKETS_SECONDS.into())
                .with_description("HTTP request duration in seconds")
                .build();

            Ok(Self {
                registry,
                _meter_provider: meter_provider,
                request_duration,
            })
        }

        fn render_prometheus(&self) -> anyhow::Result<String> {
            let metric_families = self.registry.gather();
            let mut encoded = Vec::new();
            TextEncoder::new().encode(&metric_families, &mut encoded)?;
            Ok(String::from_utf8(encoded)?)
        }

        fn record_request(
            &self,
            method: &str,
            route: Option<&str>,
            status: u16,
            duration: Duration,
        ) {
            let mut attrs = vec![
                KeyValue::new("http.request.method", method.to_owned()),
                KeyValue::new("http.response.status_code", i64::from(status)),
            ];
            if let Some(route) = route {
                attrs.push(KeyValue::new("http.route", route.to_owned()));
            }

            self.request_duration.record(duration.as_secs_f64(), &attrs);
        }
    }

    #[derive(Clone)]
    struct HttpMetricsLayer {
        observability: Observability,
    }

    impl HttpMetricsLayer {
        fn new(observability: Observability) -> Self {
            Self { observability }
        }
    }

    impl<S> Layer<S> for HttpMetricsLayer {
        type Service = HttpMetricsService<S>;

        fn layer(&self, inner: S) -> Self::Service {
            HttpMetricsService {
                inner,
                observability: self.observability.clone(),
            }
        }
    }

    #[derive(Clone)]
    struct HttpMetricsService<S> {
        inner: S,
        observability: Observability,
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
                .map(str::to_owned);
            let observability = self.observability.clone();
            let mut inner = self.inner.clone();

            Box::pin(async move {
                let response = inner.call(request).await?;

                if route.as_deref() != Some("/metrics") {
                    observability.record_http_request(
                        &method,
                        route.as_deref(),
                        response.status().as_u16(),
                        started_at.elapsed(),
                    );
                }

                Ok(response)
            })
        }
    }

    async fn metrics_endpoint(
        headers: HeaderMap,
        metrics_token: Arc<str>,
        observability: Observability,
    ) -> impl IntoResponse {
        let authorized = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|token| token == metrics_token.as_ref());

        if !authorized {
            return (
                StatusCode::UNAUTHORIZED,
                [(
                    axum::http::header::WWW_AUTHENTICATE,
                    "Bearer realm=\"metrics\"",
                )],
            )
                .into_response();
        }

        match observability.render_prometheus() {
            Ok(body) => (
                StatusCode::OK,
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; version=0.0.4; charset=utf-8",
                )],
                body,
            )
                .into_response(),
            Err(error) => {
                tracing::error!("Failed to render metrics: {error}");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

#[cfg(not(feature = "observability"))]
mod imp {
    use crate::config::ServerConfig;
    use axum::Router;

    #[derive(Clone, Default)]
    pub struct Observability;

    impl Observability {
        pub fn new(_config: &ServerConfig) -> anyhow::Result<Self> {
            Ok(Self)
        }

        pub fn apply(&self, router: Router<()>) -> Router<()> {
            router
        }
    }
}
