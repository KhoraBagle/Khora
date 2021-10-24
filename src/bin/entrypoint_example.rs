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
// use structopt::StructOpt;
use gip::{Provider, ProviderDefaultV4};

const DEFAULT_PORT: u16 = 8334;


// #[derive(Debug, Clone, StructOpt)]
// struct Opt {
//     #[structopt(long, default_value = "0.0.0.0:8334")]
//     bind_addr: SocketAddr,

//     #[structopt(long, default_value = "0.0.0.0:8334")]
//     server_addr: SocketAddr,
// }
// `35.200.46.91` is an address to connect to this server via internet.
// cargo run --bin entrypoint_example -- --bind-addr 0.0.0.0:8334 --server-addr 35.200.46.91:8334


fn main() -> Result<(), MainError> {
    // let opt = Opt::from_args();
    let logger = track!(TerminalLoggerBuilder::new().destination(Destination::Stderr).level("info".parse().unwrap()).build())?; // info or debug

        

    /* server should use local ip or 0.0.0.0 client should connect through global ip address */
    // println!("*{}", local_ipaddress::get().unwrap());
    // let addr: SocketAddr = opt.bind_addr;
    // let service = ServiceBuilder::new(addr)
    //     .logger(logger.clone())
    //     .server_addr(opt.server_addr)
    //     .finish(executor.handle(), SerialLocalNodeIdGenerator::new());
    



    let local_socket: SocketAddr = format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap();
    let mut p = ProviderDefaultV4::new();
    let global_addr = p.get_addr().unwrap().v4addr.unwrap();
    let global_socket = SocketAddr::new(IpAddr::V4(global_addr),DEFAULT_PORT);
    println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);
    let executor = track_any_err!(ThreadPoolExecutor::new())?;

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

    Ok(())
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