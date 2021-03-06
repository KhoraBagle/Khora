// #[cfg(feature = "serialize")]
// use serde_derive::{Deserialize, Serialize};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::net::SocketAddr;

/// Identifier used for distinguish local nodes in a process.
///
/// An identifier is assigned automatically to a new [`Node`] when it is created.
///
/// [`Node`]: ./struct.Node.html
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
// #[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
pub struct LocalNodeId(u64);
impl LocalNodeId {
    /// Makes a new `LocalNodeId` instance.
    pub fn new(id: u64) -> Self {
        LocalNodeId(id)
    }

    /// Returns the value of the identifier.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Identifier used for distinguish nodes in a cluster.
///
/// The identifier of a [`Node`] consists of [`LocalNodeId`] of the node and
/// the socket address of the RPC server used for communicating with the node.
///
/// [`Node`]: ./struct.Node.html
/// [`LocalNodeId`]: ./struct.LocalNodeId.html
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash)]
// #[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
pub struct NodeId {
    address: SocketAddr,
    local_id: LocalNodeId,
}
impl NodeId {
    /// Makes a new `NodeId` instance.
    pub fn new(address: SocketAddr, local_id: LocalNodeId) -> Self {
        NodeId { address, local_id }
    }

    /// Makes a new `NodeId` instance from an old one with new local_id.
    pub fn with_id(&self, local_id: u64) -> Self {
        NodeId { address: self.address, local_id: LocalNodeId::new(local_id) }
    }

    /// Returns the RPC server address part of the identifier.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Returns the local node identifier part of the identifier.
    pub fn local_id(&self) -> LocalNodeId {
        self.local_id
    }
}
impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NodeId({:?})", self.to_string())
    }
}
impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:08x}@{}", self.local_id.0, self.address)
    }
}
impl PartialOrd for NodeId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for NodeId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.address
            .ip()
            .cmp(&other.address.ip())
            .then_with(|| self.address.port().cmp(&other.address.port()))
            .then_with(|| self.local_id.cmp(&other.local_id))
    }
}
