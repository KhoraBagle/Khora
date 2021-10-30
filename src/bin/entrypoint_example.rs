#[macro_use]
extern crate trackable;


use fibers::sync::mpsc::{self, Receiver};
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::net::{IpAddr, SocketAddr};
use trackable::error::MainError;
use gip::{Provider, ProviderDefaultV4};

use natpmp::*;
use std::thread;
use std::time::Duration;


use rand::Rng;

const DEFAULT_PORT: u16 = 8334;

// systemd-resolve --status | grep Current

fn main() {
    // let opt = Opt::from_args();
    let logger = track!(TerminalLoggerBuilder::new().destination(Destination::Stderr).level("info".parse().unwrap()).build()).unwrap(); // info or debug

    let mut rng = rand::thread_rng();
    let mut n = Natpmp::new().unwrap();
    loop {
        let randport_priv = rng.gen::<u16>();
        let randport_pub = rng.gen::<u16>();
        println!("{},{}",randport_priv,randport_pub);
        n.send_public_address_request().unwrap();
        n.send_port_mapping_request(Protocol::UDP, randport_priv, randport_pub, 30).unwrap();
    

        thread::sleep(Duration::from_millis(250));
        let response = n.read_response_or_retry();
        println!("{:?}",response);
        if response.is_ok() {
            break
        }
    }

    let local_addr = "0.0.0.0".parse().unwrap();
    let local_socket = SocketAddr::new(local_addr,DEFAULT_PORT);
    let mut p = ProviderDefaultV4::new();
    let global_addr = p.get_addr().unwrap().v4addr.unwrap();
    let global_socket = SocketAddr::new(IpAddr::V4(global_addr),DEFAULT_PORT);
    println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);
    let executor = track_any_err!(ThreadPoolExecutor::new()).unwrap();

    let service = ServiceBuilder::new(local_socket)
        .logger(logger.clone())
        .server_addr(global_socket)
        .finish(executor.handle(), SerialLocalNodeIdGenerator::new());

    
    let (message_tx, message_rx) = mpsc::channel();
    let node = TestNode {
        node: NodeBuilder::new().logger(logger.clone()).finish(service.handle()),
        receiver: message_rx,
    };
    



    executor.spawn(service.map_err(|e| panic!("{}", e)));
    executor.spawn(node);


    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        // click enter to send a message or attempt to contact someone
        for line in stdin.lock().lines() {
            let line = if let Ok(line) = line {
                line.as_bytes().to_vec()
            } else {
                break;
            };
            if message_tx.send(line).is_err() {
                println!("message send was error!");
                break;
            }
        }
    });


    track_any_err!(executor.run()).unwrap();

}



/// the node used to run all the networking
struct TestNode {
    node: Node<Vec<u8>>,
    receiver: Receiver<Vec<u8>>,
}
impl Future for TestNode {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;

            while let Async::Ready(Some(msg)) = track_try_unwrap!(self.node.poll()) {
                println!("# MESSAGE: {:?}", String::from_utf8_lossy(&msg.message.payload[..]));

                println!("all plumtree peers: {:?}",self.node.plumtree_node().all_push_peers());
                self.node.handle_gossip_now(msg, true);
                did_something = true;
            }
            while let Async::Ready(Some(msg)) = self.receiver.poll().expect("Never fails") {
                // this if statement is how the entrypoint runs. Type in *[ IPv4 ] here (in example is the following: "*192.168.0.101")
                if msg.get(0) == Some(&42) /* * */ { // *192.168.0.101
                    // let addr: SocketAddr = track_any_err!(format!("{}:{}",String::from_utf8_lossy(&msg[1..]),DEFAULT_PORT).parse()).unwrap();
                    let addr_str = String::from_utf8_lossy(&msg[1..]);
                    let addr: SocketAddr = if let Some(addr) = addr_str.parse().ok() {
                        addr
                    } else {
                        track_any_err!(format!("{}:{}",addr_str,DEFAULT_PORT).parse()).unwrap()
                    };
                    let nodeid = NodeId::new(addr, LocalNodeId::new(0));
                    self.node.dm("hello!".as_bytes().to_vec(),&vec![nodeid],true);
                } else {
                    self.node.broadcast(msg);
                }
                did_something = true;
            }
        }
        Ok(Async::NotReady)
    }
}