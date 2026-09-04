use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tracing::debug;

#[derive(Clone)]
pub struct KubePodResolver {
    client: reqwest::Client,
    base_url: String,
    namespace: String,
    cache: Arc<RwLock<(Instant, HashMap<String, String>)>>,
}

impl KubePodResolver {
    pub fn try_detect() -> Option<Self> {
        let token_path = Path::new("/var/run/secrets/kubernetes.io/serviceaccount/token");
        let namespace_path = Path::new("/var/run/secrets/kubernetes.io/serviceaccount/namespace");
        let ca_path = Path::new("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt");

        let token = if token_path.exists() {
            std::fs::read_to_string(token_path).ok()?.trim().to_string()
        } else if let Ok(t) = std::env::var("KUBERNETES_TOKEN") {
            t.trim().to_string()
        } else {
            return None;
        };

        let namespace = if namespace_path.exists() {
            std::fs::read_to_string(namespace_path)
                .unwrap_or_else(|_| "bank-cluster".to_string())
                .trim()
                .to_string()
        } else {
            std::env::var("KUBERNETES_NAMESPACE").unwrap_or_else(|_| "bank-cluster".to_string())
        };

        let host = std::env::var("KUBERNETES_SERVICE_HOST")
            .unwrap_or_else(|_| "kubernetes.default.svc".to_string());
        let port = std::env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".to_string());
        let base_url = format!("https://{host}:{port}");

        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {token}")) {
            headers.insert(AUTHORIZATION, val);
        }

        let mut builder = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_millis(1500));

        if ca_path.exists() {
            if let Ok(ca_bytes) = std::fs::read(ca_path)
                && let Ok(cert) = reqwest::Certificate::from_pem(&ca_bytes) {
                    builder = builder.add_root_certificate(cert);
                }
        } else {
            builder = builder.danger_accept_invalid_certs(true);
        }

        let client = builder.build().ok()?;
        Some(Self {
            client,
            base_url,
            namespace,
            cache: Arc::new(RwLock::new((Instant::now() - Duration::from_secs(60), HashMap::new()))),
        })
    }

    /// Resolves all pod IPs in the namespace to their pod names.
    pub async fn resolve_all(&self) -> HashMap<String, String> {
        {
            let (last_updated, ref map) = *self.cache.read().await;
            if last_updated.elapsed() < Duration::from_secs(5) && !map.is_empty() {
                return map.clone();
            }
        }

        let url = format!("{}/api/v1/namespaces/{}/pods", self.base_url, self.namespace);
        let resp = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                debug!(target: "admin::k8s", "Failed to query Kubernetes pods: {e}");
                return self.cache.read().await.1.clone();
            }
        };

        if !resp.status().is_success() {
            return self.cache.read().await.1.clone();
        }

        let body: Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => return self.cache.read().await.1.clone(),
        };

        let mut map = HashMap::new();
        if let Some(items) = body["items"].as_array() {
            for item in items {
                if let (Some(name), Some(ip)) = (
                    item["metadata"]["name"].as_str(),
                    item["status"]["podIP"].as_str(),
                ) {
                    map.insert(ip.to_string(), name.to_string());
                }
            }
        }

        let mut write = self.cache.write().await;
        *write = (Instant::now(), map.clone());
        map
    }
}
