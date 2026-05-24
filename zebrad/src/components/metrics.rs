//! An HTTP endpoint for metrics collection.

#![allow(non_local_definitions)]

use std::net::SocketAddr;

use abscissa_core::{Component, FrameworkError};
use serde::{Deserialize, Serialize};

/// Abscissa component which runs a metrics endpoint.
#[derive(Debug, Component)]
pub struct MetricsEndpoint {}

impl MetricsEndpoint {
    /// Create the component.
    #[cfg(feature = "prometheus")]
    pub fn new(config: &Config) -> Result<Self, FrameworkError> {
        if let Some(addr) = config.endpoint_addr {
            info!("Trying to open metrics endpoint at {}...", addr);

            let endpoint_result = metrics_exporter_prometheus::PrometheusBuilder::new()
                .with_http_listener(addr)
                .install();

            match endpoint_result {
                Ok(()) => {
                    info!("Opened metrics endpoint at {}", addr);

                    // Expose binary metadata to metrics, using a single time series with
                    // value 1:
                    //     https://www.robustperception.io/exposing-the-software-version-to-prometheus
                    metrics::counter!(
                        format!("{}.build.info", env!("CARGO_PKG_NAME")),
                        "version" => env!("CARGO_PKG_VERSION")
                    )
                    .increment(1);
                }
                Err(e) => panic!(
                    "Opening metrics endpoint listener {addr:?} failed: {e:?}. \
                     Hint: Check if another zebrad or zcashd process is running. \
                     Try changing the metrics endpoint_addr in the Zebra config.",
                ),
            }
        }

        Ok(Self {})
    }

    /// Create the component.
    #[cfg(not(feature = "prometheus"))]
    pub fn new(config: &Config) -> Result<Self, FrameworkError> {
        if let Some(addr) = config.endpoint_addr {
            warn!(
                ?addr,
                "unable to activate configured metrics endpoint: \
                 enable the 'prometheus' feature when compiling zebrad",
            );
        }

        Ok(Self {})
    }
}

/// Metrics configuration section.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// The address used for the Prometheus metrics endpoint.
    ///
    /// Install Zebra using `cargo install --features=prometheus` to enable this config.
    ///
    /// The endpoint is disabled if this is set to `None`.
    pub endpoint_addr: Option<SocketAddr>,
}

// we like our default configs to be explicit
#[allow(unknown_lints)]
#[allow(clippy::derivable_impls)]
impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint_addr: None,
        }
    }
}

#[cfg(all(test, feature = "prometheus"))]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        time::Duration,
    };

    use metrics::{Key, Recorder};
    use metrics_exporter_prometheus::PrometheusBuilder;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        time::timeout,
    };

    static METADATA: metrics::Metadata =
        metrics::Metadata::new(module_path!(), metrics::Level::INFO, Some(module_path!()));

    async fn available_local_addr() -> SocketAddr {
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .expect("test should bind a local ephemeral port");
        let addr = listener
            .local_addr()
            .expect("bound test listener should have a local address");
        drop(listener);

        addr
    }

    #[tokio::test]
    async fn prometheus_metrics_connection_waits_without_request_timeout_today() {
        let addr = available_local_addr().await;
        let (recorder, exporter) = PrometheusBuilder::new()
            .with_http_listener(addr)
            .build()
            .expect("test metrics exporter should build");
        let exporter_task = tokio::spawn(exporter);

        let key = Key::from_name("metrics_endpoint_idle_connection_probe");
        let gauge = recorder.register_gauge(&key, &METADATA);
        gauge.set(1.0);

        let mut stream = timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(addr))
            .await
            .expect("connect timeout")
            .expect("connect ok");

        let mut first_byte = [0; 1];
        let idle_read = timeout(Duration::from_millis(250), stream.read(&mut first_byte)).await;
        assert!(
            idle_read.is_err(),
            "idle metrics connection should remain open without a request timeout today",
        );

        let request = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        timeout(Duration::from_secs(2), stream.write_all(request.as_bytes()))
            .await
            .expect("write timeout")
            .expect("write ok");

        let mut response = Vec::new();
        timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
            .await
            .expect("read timeout")
            .expect("read ok");

        let response = String::from_utf8_lossy(&response);
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "metrics exporter should accept a later scrape on the same idle connection: {response}",
        );
        assert!(
            response.contains("metrics_endpoint_idle_connection_probe"),
            "metrics exporter should render the probe metric after the idle wait: {response}",
        );

        exporter_task.abort();
    }
}
