//! SWIM-based membership and failure detector service for the Aaron Node framework.
//!
//! Manages cluster membership, node discovery, failure detection (Ping/Ack/PingReq),
//! and state dissemination via epidemic gossip protocols over QUIC with P2P TLS.

pub mod config;
pub mod error;
pub mod event;
pub mod handle;
pub mod member;
pub mod message;
pub mod proto;
pub mod service;
pub mod stage;
pub mod table;

pub use config::MembershipConfig;
pub use error::MembershipError;
pub use event::{JoinClusterCommand, MembershipEvent, UpdateSwimConfig};
pub use handle::MembershipHandle;
pub use member::{Member, MemberStatus};
pub use message::{Message, MessageError};
pub use service::MembershipService;
pub use stage::{EgressTransport, IngressHandler, ProbeLoop};
pub use table::{MembershipChange, MembershipTable};
