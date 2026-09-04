# Aaron Node Service Subsystem (`service`)

O módulo `service` define o modelo de execução, isolamento, supervisão e configuração tipada dos serviços em background gerenciados pelo daemon do nó (**Aaron Node**).

---

## 1. Visão Geral e Arquitetura

Cada serviço em execução no nó implementa a trait [`Service`](./service_trait.rs), que combina:
- **Configuração Declarativa Tipada (`ServiceConfig`)**: Cada serviço descreve as variáveis de ambiente que consome, seus tipos, descrições, valores padrão e obrigatoriedade.
- **Validação *Fail-Fast***: O `Node` inspeciona os schemas de todos os serviços registrados antes de subir a persistência ou sockets de rede, abortando imediatamente caso falte alguma variável obrigatória ou ocorra erro de tipagem.
- **Injeção Unificada de Dependências (`Context`)**: Ao ser iniciado, cada serviço recebe uma instância clonada do `Context`, com acesso a todos os 5 subsistemas essenciais do nó.
- **Supervisão e Isolamento de Falhas (`ServiceOpts`)**: Políticas de reinicialização configuráveis com estratégias de backoff e proteção contra travamentos ou pânicos.

```
                  ┌──────────────────────────────────────────────┐
                  │                 Aaron Node                   │
                  └──────────────────────┬───────────────────────┘
                                         │
                 1. validate_env() (Fail-Fast Schema Check)
                 2. generate_env_example() / .env.example
                 3. Assemble Context (Store, Network, EventHub, NodeId, Env)
                                         │
                 ┌───────────────────────┼───────────────────────┐
                 ▼                       ▼                       ▼
       ┌───────────────────┐   ┌───────────────────┐   ┌───────────────────┐
       │   Supervised Svc  │   │   Supervised Svc  │   │   Supervised Svc  │
       │  (P2P Discovery)  │   │   (QUIC Router)   │   │   (Worker Pool)   │
       │                   │   │                   │   │                   │
       │ service.run(ctx)  │   │ service.run(ctx)  │   │ service.run(ctx)  │
       └───────────────────┘   └───────────────────┘   └───────────────────┘
```

---

## 2. Componentes Principais

### A. [`Service`](./service_trait.rs)
O contrato fundamental para serviços em background:

```rust
pub trait Service: Send + Sync + 'static {
    /// Tipo de configuração do serviço (fornece schema() e from_env(env))
    type Config: ServiceConfig;

    /// Identificador legível do serviço
    fn name(&self) -> &str;

    /// Loop de execução assíncrono sob supervisão
    fn run(&self, ctx: Context) -> impl Future<Output = Result<(), BoxError>> + Send;
}
```

---

### B. [`ServiceConfig`](./config.rs) e [`ConfigField`](./config.rs)
Permite ao serviço declarar seu esquema de configuração e como se auto-instanciar a partir do [`Env`](../env.rs):

```rust
pub struct ConfigField {
    pub name: &'static str,
    pub type_name: &'static str,
    pub required: bool,
    pub default: Option<&'static str>,
    pub description: &'static str,
}

pub trait ServiceConfig: Sized + Send + Sync + 'static {
    fn schema() -> Vec<ConfigField>;
    fn from_env(env: &Env) -> Result<Self, ConfigError>;
}
```

> **Nota:** Para serviços que não dependem de variáveis de ambiente, utilize `type Config = ();`.

---

### C. [`Context`](./context.rs)
O handle compartilhado injetado em cada invocação de `run()`:

| Campo | Tipo | Descrição |
| :--- | :--- | :--- |
| `ctx.event_hub` | `EventHub` | Barramento Pub/Sub in-memory fortemente tipado via `crossfire`. |
| `ctx.network` | `Network` | Gestor multi-transporte com pool de conexões (TCP, UDP e QUIC P2P TLS). |
| `ctx.store` | `Store` | Motor de armazenamento persistente LSM-tree (Fjall 3.1) com namespaces. |
| `ctx.identity` | `NodeId` | Identidade criptográfica única de 128 bits e timestamp de incarnation. |
| `ctx.env` | `Arc<Env>` | Rastreamento e detecção de variáveis de ambiente e rede local. |

---

### D. [`ServiceOpts`](./service_opts.rs) (Supervisão e Resiliência)
Define como o supervisor deve reagir quando um serviço encerra ou sofre pânico:

- **[`RestartPolicy`](./service_opts.rs)**:
  - `Never`: Executa uma única vez; se falhar ou terminar, não reinicia.
  - `Always`: Reinicia indefinidamente (útil para daemons essenciais).
  - `OnFailure`: Reinicia apenas quando retorna `Err` ou sofre pânico.
  - `MaxRetries(n)` / `OnFailureMaxRetries(n)`: Limita o número máximo de tentativas.
- **[`BackoffStrategy`](./service_opts.rs)**:
  - `None`: Reinicia imediatamente.
  - `Constant(Duration)`: Intervalo fixo entre tentativas.
  - `Linear { base, step, max }`: Incremento linear do atraso.
  - `Exponential { base, factor, max }`: Multiplicação exponencial (ex: 1s, 2s, 4s, 8s...).

---

### E. [`service_fn`](./anon_service.rs) (Serviços Anônimos)
Permite registrar closures e funções assíncronas diretamente sem criar uma struct dedicada:

```rust
use aaron_core::{service_fn, Context, Node};

let node = Node::new().with(service_fn("metrics_reporter", |ctx: Context| async move {
    let hostname = &ctx.env.hostname;
    println!("Node {hostname} running...");
    Ok(())
}));
```

---

## 3. Exemplos de Uso

### Criando um Serviço com Configuração Tipada

```rust
use aaron_core::{
    BoxError, ConfigError, ConfigField, Context, Env, Node, Service, ServiceConfig, ServiceOpts,
};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct P2pConfig {
    pub listen_port: u16,
    pub max_peers: usize,
    pub cluster: String,
}

impl ServiceConfig for P2pConfig {
    fn schema() -> Vec<ConfigField> {
        vec![
            ConfigField::new("P2P_LISTEN_PORT", "u16")
                .required()
                .description("Porta de escuta P2P QUIC"),
            ConfigField::new("P2P_MAX_PEERS", "usize")
                .default("50")
                .description("Limite máximo de peers conectados"),
            ConfigField::new("CLUSTER_NAME", "String")
                .default("aaron-main")
                .description("Nome do cluster"),
        ]
    }

    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        Ok(Self {
            listen_port: env.get("P2P_LISTEN_PORT").ok_or_else(|| ConfigError::MissingRequired {
                service: "p2p_discovery".to_string(),
                var_name: "P2P_LISTEN_PORT".to_string(),
                description: "Porta de escuta P2P QUIC".to_string(),
            })?,
            max_peers: env.get("P2P_MAX_PEERS").unwrap_or(50),
            cluster: env.get("CLUSTER_NAME").unwrap_or_else(|| "aaron-main".to_string()),
        })
    }
}

pub struct P2pDiscoveryService;

impl Service for P2pDiscoveryService {
    type Config = P2pConfig;

    fn name(&self) -> &str {
        "p2p_discovery"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        let config = P2pConfig::from_env(&ctx.env)?;
        
        let socket = ctx.network.udp.bind(format!("0.0.0.0:{}", config.listen_port)).await?;
        println!("Discovery ouvindo na porta {}", socket.local_addr()?);

        // Loop principal do serviço
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}
```

---

### Registrando e Executando no Nó

```rust
#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let node = Node::new()
        .with_dir_path("./data")
        .with_opts(
            P2pDiscoveryService,
            ServiceOpts::new()
                .restart_on_failure()
                .exponential_backoff(Duration::from_secs(1), 2.0, Duration::from_secs(30)),
        );

    // Gera o template .env.example automaticamente
    node.write_env_example(".env.example")?;

    // Valida variáveis declaradas e executa o nó
    node.run().await
}
```

---

### Geração Automática do `.env.example`

O método `node.generate_env_example()` / `node.write_env_example()` gera o arquivo documentado:

```ini
# ==============================================================================
# Auto-generated .env.example for Aaron Node
# ==============================================================================

# === [p2p_discovery] ===
# Porta de escuta P2P QUIC
# Type: u16 (Required)
P2P_LISTEN_PORT=

# Limite máximo de peers conectados
# Type: usize (Optional, default: 50)
P2P_MAX_PEERS=50

# Nome do cluster
# Type: String (Optional, default: aaron-main)
CLUSTER_NAME=aaron-main
```
