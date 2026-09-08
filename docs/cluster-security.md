# Cluster Membership Security

## Trust Model

`cluster_id` is a random 128-bit UUID and acts as the shared secret for cluster membership.
It must be provisioned out of band and kept protected by the cluster operators. It is not
embedded in node certificates.

A node joins through a short-lived HMAC proof. The proof binds the joining node UUID, its
incarnation and a timestamp to the `cluster_id`. The default acceptance window is 2 seconds
and can be configured with `MEMBERSHIP_JOIN_AUTH_WINDOW_MS`. There is intentionally no replay
cache: a captured proof can be reused only while its configured time window remains valid.

After successful admission, the node participates in normal membership gossip. The existing
admin panel authentication policy is unchanged.

## Join Configuration

Set the shared UUID on every node through `MEMBERSHIP_CLUSTER_ID`. The value must be the same
on all members and should not be logged or placed in certificates. The existing admin panel is
unchanged by this implementation.

`MEMBERSHIP_JOIN_AUTH_WINDOW_MS` defaults to `2000`. Use a larger value only when deployment
latency requires it; a shorter value reduces the validity period of captured join proofs.

The QUIC certificate identifies the node transport endpoint. It does not establish cluster
membership by itself. Membership authorization happens at the application protocol layer
during join.

## Coordinated Upgrade

This revision uses QUIC ALPN `aaron-p2p/2` and control-plane message version 1. Deploy the
same revision to every participant before changing the membership protocol.

1. Provision the same 128-bit cluster UUID to the intended members.
2. Configure the join-authentication window where needed.
3. Deploy the new binaries and restart nodes in a controlled maintenance window.
4. Check membership, quorum and replicated state before resuming writes.

Existing FlatBuffers membership records without the new configuration sets are read as legacy
single configurations. Information already lost by an older joint-consensus serialization cannot
be reconstructed automatically. Upgrade from a stable membership, not during an in-progress
membership change.

## Snapshot Durability

Votes, logs, applied state and snapshots use journal-synchronized batches. Errors propagate to
OpenRaft and do not advance the in-memory state.

Snapshot data, applied position, membership and current snapshot metadata are installed in one
atomic batch after validating the entire payload.
