use aaron::{
    admin::{AdminConfig, AdminService},
    control_plane::{ControlPlaneConfig, ControlPlaneService},
    membership::{MembershipConfig, MembershipService},
    shard::{ShardConfig, ShardService},
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
    pub raft_port: Option<u16>,
    pub admin_port: Option<u16>,
    pub role: String,
    pub is_control_plane: bool,
    pub status: Arc<RwLock<String>>,
    pub dir_path: PathBuf,
    pub cancel_token: Arc<RwLock<CancellationToken>>,
    pub cluster_id: Uuid,
    pub seed_port: Option<u16>,
    pub tags: Vec<String>,
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

        let (membership, mem_handle) = MembershipService::pair_with_config(mem_config);

        let mut node = Node::new(&self.name)
            .with_dir_path(&self.dir_path)
            .with_cancel_token(token)
            .with_tags(self.tags.clone())
            .with(TracingService::new())
            .with(membership);

        if self.is_control_plane {
            let raft_p = self.raft_port.unwrap_or(self.quic_port + 1000);
            let raft_bind_addr = format!("127.0.0.1:{}", raft_p);
            let cp_config = ControlPlaneConfig {
                bind_addr: raft_bind_addr.parse().unwrap(),
                node_id: None,
                election_timeout_min_ms: 150,
                election_timeout_max_ms: 300,
                heartbeat_interval_ms: 40,
                snapshot_threshold: 500,
            };
            let (cp_svc, cp_handle) = ControlPlaneService::pair_with_config(cp_config);
            let (shard_svc, shard_handle) = ShardService::coordinator(cp_handle.clone());
            let shard_svc = shard_svc.with_config(ShardConfig {
                total_shards: 16,
                replication_factor: 3,
                is_coordinator: true,
            });

            node = node.with(cp_svc).with(shard_svc);

            // Notice: Control Plane nodes start UNINITIALIZED.
            // Quorum bootstrap is initiated via the Admin Console or initialization API.

            if let Some(admin_p) = self.admin_port {
                let admin_config = AdminConfig {
                    bind_addr: format!("127.0.0.1:{}", admin_p).parse().unwrap(),
                    enabled: true,
                    static_dir: None,
                };
                let admin_svc = AdminService::with_config(admin_config)
                    .with_membership_handle(mem_handle)
                    .with_control_plane_handle(cp_handle)
                    .with_shard_handle(shard_handle);

                node = node.with(admin_svc).with(service_fn("demo-seeder", |ctx: Context| async move {
                    let ks = ctx.store.keyspace("demo")?;
                    ks.insert("cluster/name", "Aaron Live Demo")?;
                    ks.insert("cluster/topology", "3 Control Plane Nodes + 3 Worker Nodes")?;
                    ks.insert("cluster/protocol", "SWIM Gossip + QUIC Multi-Stream")?;
                    ks.insert("cluster/storage", "Fjall LSM Tree")?;
                    ks.insert("cluster/consensus", "Awaiting Quorum Bootstrap via Admin Console")?;
                    ks.insert("stats/state", "3 Control Plane Nodes & 3 Workers online")?;
                    ctx.store.persist()?;
                    info!("Seeded demo keyspace in Aaron Store");
                    Ok(())
                }));
            }
        } else {
            // Worker node executes workloads & shards
            node = node.with(service_fn("worker-workload", |ctx: Context| async move {
                let ks = ctx.store.keyspace("workload")?;
                ks.insert("node_role", "Worker")?;
                ks.insert("status", "Active Shard Processor")?;
                ctx.store.persist()?;
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

#[derive(Clone, Debug)]
pub struct ClientSessionRecord {
    pub client_id: String,
    pub active_session_id: Option<String>,
    pub created_at: Instant,
    pub expires_at: Instant,
}

pub enum ClusterError {
    AlreadyActive(Arc<DemoCluster>),
    Cooldown { remaining_secs: u64 },
    SlotsFull(usize),
    Other(String),
}

pub struct DemoCluster {
    pub client_id: String,
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(350))
            .build()
            .unwrap_or_default();

        let cp_info: Option<serde_json::Value> = match client
            .get(format!("http://127.0.0.1:{}/api/control-plane/status", self.admin_port))
            .send()
            .await
        {
            Ok(resp) => resp.json::<serde_json::Value>().await.ok(),
            Err(_) => None,
        };

        let is_raft_initialized = cp_info.as_ref().map(|cp| {
            let has_voters = cp.get("voters").and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false);
            let term = cp.get("current_term").and_then(|t| t.as_u64()).unwrap_or(0);
            has_voters || term > 0
        }).unwrap_or(false);

        let current_leader = cp_info.as_ref().and_then(|cp| cp.get("current_leader").and_then(|l| l.as_u64()));

        let leader_raft_port = cp_info.as_ref().and_then(|cp| {
            let leader_id = cp.get("current_leader").and_then(|l| l.as_u64())?;
            let nodes = cp.get("nodes")?.as_object()?;
            for (_, v) in nodes {
                if v.get("node_id")?.as_u64() == Some(leader_id) {
                    let addr = v.get("addr")?.as_str()?;
                    let port = addr.split(':').last()?.parse::<u16>().ok()?;
                    return Some(port);
                }
            }
            None
        });

        let mut node_summaries = Vec::new();
        for n in &self.nodes {
            let status = n.status.read().await.clone();
            let raft_role = if n.is_control_plane {
                if !is_raft_initialized {
                    "Uninitialized".to_string()
                } else if n.raft_port.is_some() && n.raft_port == leader_raft_port {
                    "Leader".to_string()
                } else {
                    "Follower".to_string()
                }
            } else {
                "Worker".to_string()
            };

            node_summaries.push(serde_json::json!({
                "id": n.id,
                "name": n.name,
                "quic_port": n.quic_port,
                "raft_port": n.raft_port,
                "admin_port": n.admin_port,
                "role": n.role,
                "raft_role": raft_role,
                "is_control_plane": n.is_control_plane,
                "status": status,
            }));
        }

        let remaining_secs = self.expires_at.saturating_duration_since(Instant::now()).as_secs();

        serde_json::json!({
            "session_id": self.session_id,
            "cluster_id": self.cluster_id.to_string(),
            "admin_port": self.admin_port,
            "nodes": node_summaries,
            "is_raft_initialized": is_raft_initialized,
            "current_leader": current_leader,
            "control_plane_status": cp_info,
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
    client_records: Arc<RwLock<HashMap<String, ClientSessionRecord>>>,
    used_ports: Arc<RwLock<HashSet<u16>>>,
    max_clusters: usize,
    ttl: Duration,
    http_client: reqwest::Client,
    start_time: Instant,
    total_clusters_created: Arc<std::sync::atomic::AtomicU64>,
    total_clusters_reaped: Arc<std::sync::atomic::AtomicU64>,
    total_benchmarks_run: Arc<std::sync::atomic::AtomicU64>,
}

impl DemoClusterManager {
    pub fn new(max_clusters: usize, ttl: Duration) -> Self {
        Self {
            clusters: Arc::new(RwLock::new(HashMap::new())),
            client_records: Arc::new(RwLock::new(HashMap::new())),
            used_ports: Arc::new(RwLock::new(HashSet::new())),
            max_clusters,
            ttl,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            start_time: Instant::now(),
            total_clusters_created: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            total_clusters_reaped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            total_benchmarks_run: Arc::new(std::sync::atomic::AtomicU64::new(0)),
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

    fn find_available_port_pair(
        used: &HashSet<u16>,
        start: u16,
        end: u16,
        offset: u16,
    ) -> Option<(u16, u16)> {
        for p in start..end {
            let p2 = p + offset;
            if used.contains(&p) || used.contains(&p2) {
                continue;
            }
            if let Ok(sock1) = UdpSocket::bind(("127.0.0.1", p)) {
                drop(sock1);
                if let Ok(sock2) = UdpSocket::bind(("127.0.0.1", p2)) {
                    drop(sock2);
                    return Some((p, p2));
                }
            }
        }
        None
    }

    pub async fn create_cluster_for_client(&self, client_id: &str) -> Result<Arc<DemoCluster>, ClusterError> {
        let now = Instant::now();

        // 1. Check if this client already has an active cluster or is still in the 15-min cooldown
        {
            let records = self.client_records.read().await;
            if let Some(record) = records.get(client_id) {
                if now < record.expires_at {
                    let remaining_secs = record.expires_at.saturating_duration_since(now).as_secs();
                    if let Some(ref sid) = record.active_session_id {
                        let clusters = self.clusters.read().await;
                        if let Some(c) = clusters.get(sid) {
                            return Err(ClusterError::AlreadyActive(c.clone()));
                        }
                    }
                    return Err(ClusterError::Cooldown { remaining_secs });
                }
            }
        }

        let mut clusters = self.clusters.write().await;
        if clusters.len() >= self.max_clusters {
            return Err(ClusterError::SlotsFull(self.max_clusters));
        }

        let mut used = self.used_ports.write().await;

        // Allocate ports for 3 Control Plane nodes (CP-1, CP-2, CP-3)
        // Raft port MUST be swim_port + 1000 to match aaron-admin's derive_cp_port()
        let (u1, raft1) = Self::find_available_port_pair(&used, 18100, 23000, 1000)
            .ok_or_else(|| ClusterError::Other("Failed to allocate UDP ports for CP-1".to_string()))?;
        used.insert(u1);
        used.insert(raft1);

        let (u2, raft2) = Self::find_available_port_pair(&used, 18100, 23000, 1000)
            .ok_or_else(|| ClusterError::Other("Failed to allocate UDP ports for CP-2".to_string()))?;
        used.insert(u2);
        used.insert(raft2);

        let (u3, raft3) = Self::find_available_port_pair(&used, 18100, 23000, 1000)
            .ok_or_else(|| ClusterError::Other("Failed to allocate UDP ports for CP-3".to_string()))?;
        used.insert(u3);
        used.insert(raft3);

        // Allocate UDP ports for 3 Worker nodes (Worker 1, Worker 2, Worker 3)
        let u4 = Self::find_available_port(&used, 18100, 25000, true)
            .ok_or_else(|| ClusterError::Other("Failed to allocate UDP port for Worker 1".to_string()))?;
        used.insert(u4);

        let u5 = Self::find_available_port(&used, 18100, 25000, true)
            .ok_or_else(|| ClusterError::Other("Failed to allocate UDP port for Worker 2".to_string()))?;
        used.insert(u5);

        let u6 = Self::find_available_port(&used, 18100, 25000, true)
            .ok_or_else(|| ClusterError::Other("Failed to allocate UDP port for Worker 3".to_string()))?;
        used.insert(u6);

        // Allocate TCP port for CP-1 Admin Console
        let admin_port = Self::find_available_port(&used, 28100, 35000, false)
            .ok_or_else(|| ClusterError::Other("Failed to allocate TCP port for Admin Console".to_string()))?;
        used.insert(admin_port);

        let session_id = format!("cluster-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let cluster_id = Uuid::random();
        let root_dir = std::env::temp_dir().join(format!("aaron-demo-{}", session_id));
        let _ = std::fs::create_dir_all(&root_dir);

        let cp_tags = vec!["role:control-plane".into(), "control-plane".into()];
        let worker_tags = vec!["role:worker".into(), "worker".into()];

        let cp1 = Arc::new(DemoNode {
            id: 1,
            name: "aaron-cp-1 (Control Plane)".to_string(),
            quic_port: u1,
            raft_port: Some(raft1),
            admin_port: Some(admin_port),
            role: "Control Plane".to_string(),
            is_control_plane: true,
            status: Arc::new(RwLock::new("starting".to_string())),
            dir_path: root_dir.join("control-plane-1"),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            cluster_id,
            seed_port: None,
            tags: cp_tags.clone(),
        });

        let cp2 = Arc::new(DemoNode {
            id: 2,
            name: "aaron-cp-2 (Control Plane)".to_string(),
            quic_port: u2,
            raft_port: Some(raft2),
            admin_port: None,
            role: "Control Plane".to_string(),
            is_control_plane: true,
            status: Arc::new(RwLock::new("starting".to_string())),
            dir_path: root_dir.join("control-plane-2"),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            cluster_id,
            seed_port: Some(u1),
            tags: cp_tags.clone(),
        });

        let cp3 = Arc::new(DemoNode {
            id: 3,
            name: "aaron-cp-3 (Control Plane)".to_string(),
            quic_port: u3,
            raft_port: Some(raft3),
            admin_port: None,
            role: "Control Plane".to_string(),
            is_control_plane: true,
            status: Arc::new(RwLock::new("starting".to_string())),
            dir_path: root_dir.join("control-plane-3"),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            cluster_id,
            seed_port: Some(u1),
            tags: cp_tags,
        });

        let worker1 = Arc::new(DemoNode {
            id: 4,
            name: "aaron-worker-1 (Worker)".to_string(),
            quic_port: u4,
            raft_port: None,
            admin_port: None,
            role: "Worker".to_string(),
            is_control_plane: false,
            status: Arc::new(RwLock::new("starting".to_string())),
            dir_path: root_dir.join("worker-1"),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            cluster_id,
            seed_port: Some(u1),
            tags: worker_tags.clone(),
        });

        let worker2 = Arc::new(DemoNode {
            id: 5,
            name: "aaron-worker-2 (Worker)".to_string(),
            quic_port: u5,
            raft_port: None,
            admin_port: None,
            role: "Worker".to_string(),
            is_control_plane: false,
            status: Arc::new(RwLock::new("starting".to_string())),
            dir_path: root_dir.join("worker-2"),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            cluster_id,
            seed_port: Some(u1),
            tags: worker_tags.clone(),
        });

        let worker3 = Arc::new(DemoNode {
            id: 6,
            name: "aaron-worker-3 (Worker)".to_string(),
            quic_port: u6,
            raft_port: None,
            admin_port: None,
            role: "Worker".to_string(),
            is_control_plane: false,
            status: Arc::new(RwLock::new("starting".to_string())),
            dir_path: root_dir.join("worker-3"),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            cluster_id,
            seed_port: Some(u1),
            tags: worker_tags,
        });

        // Start 3 Control Plane nodes and 3 Worker nodes
        cp1.start().await;
        cp2.start().await;
        cp3.start().await;
        worker1.start().await;
        worker2.start().await;
        worker3.start().await;

        let cluster = Arc::new(DemoCluster {
            client_id: client_id.to_string(),
            session_id: session_id.clone(),
            cluster_id,
            admin_port,
            nodes: vec![cp1, cp2, cp3, worker1, worker2, worker3],
            created_at: now,
            expires_at: now + self.ttl,
            root_dir,
        });

        clusters.insert(session_id.clone(), cluster.clone());

        // Save client session record
        {
            let mut records = self.client_records.write().await;
            records.insert(
                client_id.to_string(),
                ClientSessionRecord {
                    client_id: client_id.to_string(),
                    active_session_id: Some(session_id.clone()),
                    created_at: now,
                    expires_at: now + self.ttl,
                },
            );
        }

        info!(
            client_id = %client_id,
            session_id = %session_id,
            admin_port = %admin_port,
            "Spawned Aaron demo topology: 3 Control Plane nodes (CP-1, CP-2, CP-3) & 3 Worker nodes (W-1, W-2, W-3)"
        );

        self.total_clusters_created.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(cluster)
    }

    pub async fn get_cluster(&self, session_id: &str) -> Option<Arc<DemoCluster>> {
        let clusters = self.clusters.read().await;
        clusters.get(session_id).cloned()
    }

    pub async fn get_cluster_for_client(&self, client_id: &str) -> Option<Arc<DemoCluster>> {
        let records = self.client_records.read().await;
        if let Some(record) = records.get(client_id) {
            if let Some(ref sid) = record.active_session_id {
                let clusters = self.clusters.read().await;
                return clusters.get(sid).cloned();
            }
        }
        None
    }

    pub async fn get_client_record(&self, client_id: &str) -> Option<ClientSessionRecord> {
        let records = self.client_records.read().await;
        records.get(client_id).cloned()
    }

    pub async fn get_any_cluster(&self) -> Option<Arc<DemoCluster>> {
        let clusters = self.clusters.read().await;
        clusters.values().next().cloned()
    }

    pub async fn kill_node(&self, session_id: &str, node_idx: usize) -> Result<String, String> {
        let cluster = self.get_cluster(session_id).await.ok_or_else(|| "Session not found".to_string())?;
        let node = cluster.nodes.iter().find(|n| n.id == node_idx).ok_or_else(|| "Node not found".to_string())?;
        node.kill().await;
        if node.is_control_plane {
            Ok(format!("{} shut down. Raft consensus quorum adapting.", node.name))
        } else {
            Ok(format!("{} shut down. SWIM gossip failure detector will mark it Suspect then Dead.", node.name))
        }
    }

    pub async fn revive_node(&self, session_id: &str, node_idx: usize) -> Result<String, String> {
        let cluster = self.get_cluster(session_id).await.ok_or_else(|| "Session not found".to_string())?;
        let node = cluster.nodes.iter().find(|n| n.id == node_idx).ok_or_else(|| "Node not found".to_string())?;
        node.start().await;
        if node.is_control_plane {
            Ok(format!("{} revived. Rejoining Raft consensus quorum.", node.name))
        } else {
            Ok(format!("{} revived with incremented incarnation. Rejoining cluster via SWIM gossip.", node.name))
        }
    }

    pub async fn init_control_plane(&self, session_id: &str) -> Result<serde_json::Value, String> {
        let cluster = self.get_cluster(session_id).await.ok_or_else(|| "Session not found".to_string())?;
        let url = format!("http://127.0.0.1:{}/api/control-plane/init", cluster.admin_port);
        let resp = self.http_client
            .post(&url)
            .json(&serde_json::json!({
                "voters": []
            }))
            .send()
            .await
            .map_err(|e| format!("Init Control Plane HTTP error: {e}"))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Init Control Plane failed: {body}"));
        }

        resp.json().await.map_err(|e| format!("Failed to parse init json: {e}"))
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

        self.total_benchmarks_run.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        resp.json().await.map_err(|e| format!("Failed to parse benchmark json: {e}"))
    }

    pub async fn terminate_cluster(&self, session_id: &str) -> Result<(), String> {
        let cluster_opt = {
            let mut clusters = self.clusters.write().await;
            clusters.remove(session_id)
        };

        if let Some(cluster) = cluster_opt {
            self.total_clusters_reaped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            // Update client record to clear active_session_id while maintaining cooldown expires_at
            {
                let mut records = self.client_records.write().await;
                if let Some(rec) = records.get_mut(&cluster.client_id) {
                    rec.active_session_id = None;
                }
            }

            let mut used = self.used_ports.write().await;
            for n in &cluster.nodes {
                used.remove(&n.quic_port);
                if let Some(rp) = n.raft_port {
                    used.remove(&rp);
                }
            }
            used.remove(&cluster.admin_port);

            cluster.shutdown().await;
            info!(session_id = %session_id, client_id = %cluster.client_id, "Cleaned up and shut down Aaron demo cluster");
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

        // Clean up client records where cooldown has completely expired
        {
            let mut records = self.client_records.write().await;
            records.retain(|_, rec| now < rec.expires_at);
        }
    }

    pub async fn get_metrics_summary(&self) -> serde_json::Value {
        let uptime_secs = self.start_time.elapsed().as_secs();
        let days = uptime_secs / 86400;
        let hours = (uptime_secs % 86400) / 3600;
        let mins = (uptime_secs % 3600) / 60;
        let secs = uptime_secs % 60;
        let uptime_human = if days > 0 {
            format!("{days}d {hours}h {mins}m {secs}s")
        } else if hours > 0 {
            format!("{hours}h {mins}m {secs}s")
        } else {
            format!("{mins}m {secs}s")
        };

        let total_created = self.total_clusters_created.load(std::sync::atomic::Ordering::Relaxed);
        let total_reaped = self.total_clusters_reaped.load(std::sync::atomic::Ordering::Relaxed);
        let total_benchmarks = self.total_benchmarks_run.load(std::sync::atomic::Ordering::Relaxed);

        let active_clusters_map = self.clusters.read().await.clone();
        let active_count = active_clusters_map.len();
        let max_clusters = self.max_clusters;
        let available_slots = max_clusters.saturating_sub(active_count);

        let used_ports_count = self.used_ports.read().await.len();
        let unique_clients_count = self.client_records.read().await.len();

        let memory_rss_mb = get_memory_rss_mb().unwrap_or(0.0);

        let now = Instant::now();
        let mut active_list = Vec::new();
        for (_, cluster) in active_clusters_map {
            let summary = cluster.status_summary().await;
            let remaining = cluster.expires_at.saturating_duration_since(now).as_secs();
            let rem_mins = remaining / 60;
            let rem_secs = remaining % 60;
            let deadline_human = format!("{rem_mins:02}m {rem_secs:02}s");

            let nodes = summary.get("nodes").and_then(|n| n.as_array()).cloned().unwrap_or_default();
            let alive_nodes_count = nodes
                .iter()
                .filter(|n| n.get("status").and_then(|s| s.as_str()) == Some("running"))
                .count();

            active_list.push(serde_json::json!({
                "session_id": cluster.session_id,
                "client_id": cluster.client_id,
                "cluster_id": cluster.cluster_id.to_string(),
                "admin_port": cluster.admin_port,
                "created_secs_ago": cluster.created_at.elapsed().as_secs(),
                "ttl_remaining_secs": remaining,
                "ttl_remaining_human": deadline_human,
                "is_raft_initialized": summary.get("is_raft_initialized").and_then(|b| b.as_bool()).unwrap_or(false),
                "current_leader": summary.get("current_leader"),
                "alive_nodes_count": alive_nodes_count,
                "total_nodes_count": nodes.len(),
                "nodes": nodes,
            }));
        }

        serde_json::json!({
            "status": "online",
            "server": {
                "uptime_secs": uptime_secs,
                "uptime_human": uptime_human,
                "memory_rss_mb": (memory_rss_mb * 100.0).round() / 100.0,
                "active_ports_count": used_ports_count,
                "unique_clients_seen": unique_clients_count,
            },
            "clusters": {
                "total_created": total_created,
                "total_reaped": total_reaped,
                "active_count": active_count,
                "max_capacity": max_clusters,
                "available_slots": available_slots,
                "total_benchmarks_run": total_benchmarks,
                "active": active_list,
            }
        })
    }
}

pub fn get_memory_rss_mb() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(pages) = parts[1].parse::<u64>() {
                    let bytes = pages * 4096;
                    return Some((bytes as f64) / (1024.0 * 1024.0));
                }
            }
        }
    }
    None
}
