use aaron::{
    admin::{AdminConfig, AdminService},
    membership::{MembershipConfig, MembershipService},
    tracing::TracingService,
    Context, Node, Uuid, service_fn,
};
use std::collections::{HashMap, HashSet};
use std::net::{TcpListener, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub struct DemoNode {
    pub id: usize,
    pub name: String,
    pub quic_port: u16,
    pub admin_port: Option<u16>,
    pub role: String,
    pub status: Arc<RwLock<String>>,
    pub dir_path: PathBuf,
    pub cancel_token: Arc<RwLock<CancellationToken>>,
    pub cluster_id: Uuid,
    pub seed_port: Option<u16>,
}

impl DemoNode {
    pub async fn start(&self) {
        let token = CancellationToken::new();
        *self.cancel_token.write().await = token.clone();
        *self.status.write().await = "running".to_string();

        let quic_addr = format!("127.0.0.1:{}", self.quic_port);
        let seeds = match self.seed_port {
            Some(p) => vec![format!("127.0.0.1:{}", p)],
            None => vec![],
        };

        let mem_config = MembershipConfig {
            bind_addr: quic_addr,
            seeds,
            cluster_id: Some(self.cluster_id),
            probe_interval: Duration::from_millis(400),
            probe_timeout: Duration::from_millis(120),
            suspect_timeout: Duration::from_millis(800),
            indirect_ping_targets: 3,
            gossip_fanout: 3,
        };

        let (membership, handle) = MembershipService::pair_with_config(mem_config);

        let mut node = Node::new(&self.name)
            .with_dir_path(&self.dir_path)
            .with_cancel_token(token)
            .with(TracingService::new())
            .with(membership);

        if let Some(admin_p) = self.admin_port {
            let admin_config = AdminConfig {
                bind_addr: format!("127.0.0.1:{}", admin_p).parse().unwrap(),
                enabled: true,
                static_dir: None,
            };
            let admin_svc = AdminService::with_config(admin_config)
                .with_membership_handle(handle);

            node = node.with(admin_svc).with(service_fn("demo-seeder", |ctx: Context| async move {
                let ks = ctx.store.keyspace("demo")?;
                ks.insert("cluster/name", "Aaron Live Demo")?;
                ks.insert("cluster/protocol", "SWIM Gossip + QUIC Multi-Stream")?;
                ks.insert("cluster/storage", "Fjall LSM Tree")?;
                ks.insert("cluster/consensus", "Embedded Raft Support")?;
                ks.insert("stats/state", "All 3 nodes online and gossiping")?;
                ctx.store.persist()?;
                info!("Seeded demo keyspace in Aaron Store");
                Ok(())
            }));
        }

        tokio::spawn(async move {
            if let Err(err) = node.run().await {
                error!("Demo node terminated with error: {err}");
            }
        });
    }

    pub async fn kill(&self) {
        let token = self.cancel_token.read().await.clone();
        token.cancel();
        *self.status.write().await = "killed".to_string();
    }
}

pub struct DemoCluster {
    pub session_id: String,
    pub cluster_id: Uuid,
    pub admin_port: u16,
    pub nodes: Vec<Arc<DemoNode>>,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub root_dir: PathBuf,
}

impl DemoCluster {
    pub async fn status_summary(&self) -> serde_json::Value {
        let mut node_summaries = Vec::new();
        for n in &self.nodes {
            let status = n.status.read().await.clone();
            node_summaries.push(serde_json::json!({
                "id": n.id,
                "name": n.name,
                "quic_port": n.quic_port,
                "admin_port": n.admin_port,
                "role": n.role,
                "status": status,
            }));
        }

        let remaining_secs = self.expires_at.saturating_duration_since(Instant::now()).as_secs();

        serde_json::json!({
            "session_id": self.session_id,
            "cluster_id": self.cluster_id.to_string(),
            "admin_port": self.admin_port,
            "nodes": node_summaries,
            "ttl_remaining_secs": remaining_secs,
            "created_secs_ago": self.created_at.elapsed().as_secs(),
        })
    }

    pub async fn shutdown(&self) {
        for n in &self.nodes {
            n.kill().await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = std::fs::remove_dir_all(&self.root_dir);
    }
}

#[derive(Clone)]
pub struct DemoClusterManager {
    clusters: Arc<RwLock<HashMap<String, Arc<DemoCluster>>>>,
    used_ports: Arc<RwLock<HashSet<u16>>>,
    max_clusters: usize,
    ttl: Duration,
    http_client: reqwest::Client,
}

impl DemoClusterManager {
    pub fn new(max_clusters: usize, ttl: Duration) -> Self {
        Self {
            clusters: Arc::new(RwLock::new(HashMap::new())),
            used_ports: Arc::new(RwLock::new(HashSet::new())),
            max_clusters,
            ttl,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn max_clusters(&self) -> usize {
        self.max_clusters
    }

    pub async fn active_count(&self) -> usize {
        self.clusters.read().await.len()
    }

    fn find_available_port(used: &HashSet<u16>, start: u16, end: u16, udp: bool) -> Option<u16> {
        for p in start..end {
            if used.contains(&p) {
                continue;
            }
            if udp {
                if let Ok(sock) = UdpSocket::bind(("127.0.0.1", p)) {
                    drop(sock);
                    return Some(p);
                }
            } else if let Ok(listener) = TcpListener::bind(("127.0.0.1", p)) {
                drop(listener);
                return Some(p);
            }
        }
        None
    }

    pub async fn create_cluster(&self) -> Result<Arc<DemoCluster>, String> {
        let mut clusters = self.clusters.write().await;
        if clusters.len() >= self.max_clusters {
            return Err(format!(
                "Maximum number of concurrent demo clusters ({}) reached. Please wait for an existing session to finish.",
                self.max_clusters
            ));
        }

        let mut used = self.used_ports.write().await;
        let u1 = Self::find_available_port(&used, 18100, 25000, true)
            .ok_or_else(|| "Failed to allocate UDP port for Node 1".to_string())?;
        used.insert(u1);

        let u2 = Self::find_available_port(&used, 18100, 25000, true)
            .ok_or_else(|| "Failed to allocate UDP port for Node 2".to_string())?;
        used.insert(u2);

        let u3 = Self::find_available_port(&used, 18100, 25000, true)
            .ok_or_else(|| "Failed to allocate UDP port for Node 3".to_string())?;
        used.insert(u3);

        let admin_port = Self::find_available_port(&used, 28100, 35000, false)
            .ok_or_else(|| "Failed to allocate TCP port for Admin Console".to_string())?;
        used.insert(admin_port);

        let session_id = format!("cluster-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let cluster_id = Uuid::random();
        let root_dir = std::env::temp_dir().join(format!("aaron-demo-{}", session_id));
        let _ = std::fs::create_dir_all(&root_dir);

        let node1 = Arc::new(DemoNode {
            id: 1,
            name: "aaron-node-1 (Seed / Admin)".to_string(),
            quic_port: u1,
            admin_port: Some(admin_port),
            role: "Seed / Admin".to_string(),
            status: Arc::new(RwLock::new("starting".to_string())),
            dir_path: root_dir.join("node-1"),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            cluster_id,
            seed_port: None,
        });

        let node2 = Arc::new(DemoNode {
            id: 2,
            name: "aaron-node-2 (Worker)".to_string(),
            quic_port: u2,
            admin_port: None,
            role: "Worker".to_string(),
            status: Arc::new(RwLock::new("starting".to_string())),
            dir_path: root_dir.join("node-2"),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            cluster_id,
            seed_port: Some(u1),
        });

        let node3 = Arc::new(DemoNode {
            id: 3,
            name: "aaron-node-3 (Worker)".to_string(),
            quic_port: u3,
            admin_port: None,
            role: "Worker".to_string(),
            status: Arc::new(RwLock::new("starting".to_string())),
            dir_path: root_dir.join("node-3"),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            cluster_id,
            seed_port: Some(u1),
        });

        // Start all 3 nodes
        node1.start().await;
        node2.start().await;
        node3.start().await;

        let cluster = Arc::new(DemoCluster {
            session_id: session_id.clone(),
            cluster_id,
            admin_port,
            nodes: vec![node1, node2, node3],
            created_at: Instant::now(),
            expires_at: Instant::now() + self.ttl,
            root_dir,
        });

        clusters.insert(session_id.clone(), cluster.clone());
        info!(
            session_id = %session_id,
            admin_port = %admin_port,
            "Spawned 3-node Aaron demo cluster"
        );

        Ok(cluster)
    }

    pub async fn get_cluster(&self, session_id: &str) -> Option<Arc<DemoCluster>> {
        let clusters = self.clusters.read().await;
        clusters.get(session_id).cloned()
    }

    pub async fn get_any_cluster(&self) -> Option<Arc<DemoCluster>> {
        let clusters = self.clusters.read().await;
        clusters.values().next().cloned()
    }

    pub async fn kill_node(&self, session_id: &str, node_idx: usize) -> Result<String, String> {
        let cluster = self.get_cluster(session_id).await.ok_or_else(|| "Session not found".to_string())?;
        let node = cluster.nodes.iter().find(|n| n.id == node_idx).ok_or_else(|| "Node not found".to_string())?;
        node.kill().await;
        Ok(format!("Node {} ({}) shut down. SWIM gossip failure detector will mark it Suspect then Dead.", node.id, node.name))
    }

    pub async fn revive_node(&self, session_id: &str, node_idx: usize) -> Result<String, String> {
        let cluster = self.get_cluster(session_id).await.ok_or_else(|| "Session not found".to_string())?;
        let node = cluster.nodes.iter().find(|n| n.id == node_idx).ok_or_else(|| "Node not found".to_string())?;
        node.start().await;
        Ok(format!("Node {} ({}) revived with incremented incarnation. Rejoining cluster via SWIM gossip.", node.id, node.name))
    }

    pub async fn run_benchmark(&self, session_id: &str, operations: usize) -> Result<serde_json::Value, String> {
        let cluster = self.get_cluster(session_id).await.ok_or_else(|| "Session not found".to_string())?;
        let url = format!("http://127.0.0.1:{}/api/store/benchmark", cluster.admin_port);
        let resp = self.http_client
            .post(&url)
            .json(&serde_json::json!({
                "operations": operations.clamp(100, 5000),
                "val_size_bytes": 128,
            }))
            .send()
            .await
            .map_err(|e| format!("Benchmark HTTP error: {e}"))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Benchmark failed: {body}"));
        }

        resp.json().await.map_err(|e| format!("Failed to parse benchmark json: {e}"))
    }

    pub async fn terminate_cluster(&self, session_id: &str) -> Result<(), String> {
        let cluster_opt = {
            let mut clusters = self.clusters.write().await;
            clusters.remove(session_id)
        };

        if let Some(cluster) = cluster_opt {
            let mut used = self.used_ports.write().await;
            for n in &cluster.nodes {
                used.remove(&n.quic_port);
            }
            used.remove(&cluster.admin_port);

            cluster.shutdown().await;
            info!(session_id = %session_id, "Cleaned up and shut down Aaron demo cluster");
            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }

    pub async fn reap_expired(&self) {
        let now = Instant::now();
        let expired_ids: Vec<String> = {
            let clusters = self.clusters.read().await;
            clusters
                .iter()
                .filter(|(_, c)| now >= c.expires_at)
                .map(|(id, _)| id.clone())
                .collect()
        };

        for id in expired_ids {
            info!(session_id = %id, "Demo cluster TTL expired, reaping...");
            let _ = self.terminate_cluster(&id).await;
        }
    }
}
