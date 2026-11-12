//! Métricas no formato Prometheus para o worker.

use metrics_exporter_prometheus::{BuildError, PrometheusBuilder, PrometheusHandle};

pub fn instalar() -> Result<PrometheusHandle, BuildError> {
    let handle = PrometheusBuilder::new().install_recorder()?;
    metrics::gauge!("nexus_worker_build_info", "versao" => env!("CARGO_PKG_VERSION")).set(1.0);
    Ok(handle)
}
