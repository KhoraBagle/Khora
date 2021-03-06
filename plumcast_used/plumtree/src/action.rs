use crate::message::{GossipMessage, ProtocolMessage};
use crate::System;
use std::collections::VecDeque;
use std::fmt;

/// Actions instructed by Plumtree [Node].
///
/// For running Plumtree nodes, the actions must be handled correctly by upper layers.
///
/// [Node]: ./struct.Node.html
pub enum Action<T: System> {
    /// Send a message.
    ///
    /// If it is failed to send the message (e.g., the destination node does not exist),
    /// the message will be discarded silently.
    Send {
        /// The destination of the message.
        destination: T::NodeId,

        /// The outgoing message.
        message: ProtocolMessage<T>,
    },

    /// Deliver a message to the applications waiting for messages.
    Deliver {
        /// The message to be delivered.
        message: GossipMessage<T>,
    },
}
impl<T: System> Action<T> {
    pub(crate) fn send<M>(destination: T::NodeId, message: M) -> Self
    where
        M: Into<ProtocolMessage<T>>,
    {
        Action::Send {
            destination,
            message: message.into(),
        }
    }
}
impl<T: System> fmt::Debug for Action<T>
where
    T::NodeId: fmt::Debug,
    T::MessageId: fmt::Debug,
    T::MessagePayload: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Action::Send {
                destination,
                message,
            } => write!(
                f,
                "Send {{ destination: {:?}, message: {:?} }}",
                destination, message
            ),
            Action::Deliver { message } => write!(f, "Deliver {{ message: {:?} }}", message),
        }
    }
}

pub struct ActionQueue<T: System>(VecDeque<Action<T>>);
impl<T: System> ActionQueue<T>{
    pub fn new() -> Self {
        ActionQueue(VecDeque::new())
    }

    pub fn drain_send_actions(&mut self) -> VecDeque<Action<T>> {
        let m = self.0.iter().filter_map(|x| {
            match x {
                Action::Send { destination, message } => Some(Action::Send { destination:destination.clone(), message:message.clone() }),
                Action::Deliver { message: _ } => None,
            }
        }).collect::<VecDeque<_>>();
        self.0 = self.0.iter().filter_map(|x| {
            match x {
                Action::Send { destination: _ , message: _ } => None,
                Action::Deliver { message } => Some(Action::Deliver { message:message.clone() }),
            }
        }).collect();
        m
    }

    pub fn send_now<M: Into<ProtocolMessage<T>>>(&mut self, destination: T::NodeId, message: M) {
        self.0.push_front(Action::send(destination, message));
    }

    pub fn send<M: Into<ProtocolMessage<T>>>(&mut self, destination: T::NodeId, message: M) {
        self.0.push_back(Action::send(destination, message));
    }

    pub fn deliver_now(&mut self, message: GossipMessage<T>) {
        self.0.push_front(Action::Deliver { message });
    }

    pub fn deliver(&mut self, message: GossipMessage<T>) {
        self.0.push_back(Action::Deliver { message });
    }

    pub fn pop(&mut self) -> Option<Action<T>> {
        self.0.pop_front()
    }
}
impl<T: System> fmt::Debug for ActionQueue<T>
where
    T::NodeId: fmt::Debug,
    T::MessageId: fmt::Debug,
    T::MessagePayload: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ActionQueue({:?})", self.0)
    }
}
