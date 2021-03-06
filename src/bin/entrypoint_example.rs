#[macro_use]
extern crate trackable;


use fibers::sync::mpsc::{self, Receiver};
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use trackable::error::MainError;
use gip::{Provider, ProviderDefaultV4};
// use local_ip_address::local_ip;
use std::thread;
use std::time::Duration;
use std::io::BufRead;


use rand::Rng;

const DEFAULT_PORT: u16 = 8334;

// systemd-resolve --status | grep Current

fn main() {


    // thread::spawn(move || { // this was just checking that you can send a tcpstream between threads
    //     let outerlistener = TcpListener::bind("0.0.0.0:9999").unwrap();
    //     for stream in outerlistener.incoming() {
    //         let mut stream = stream.unwrap();
    //         println!("got stream {:?}",stream);
    //         loop {

    //             let mut m = vec![0;100];
    //             match stream.read(&mut m) {
    //                 Ok(x) => {
    //                     println!("{}:\n{:?}",x,String::from_utf8_lossy(&m));
    //                     break
    //                 }
    //                 Err(err) => {
    //                     println!("# ERROR: {}",err);
    //                 }
    //             }
    //         }
    //     }
    // });
    // thread::spawn(move || {
    //     let x = TcpStream::connect("0.0.0.0:9999").unwrap();
    //     s.send(x).unwrap();
    //     println!("sent tcpstream");
    //     thread::sleep(Duration::from_secs(1));
    //     let x = TcpStream::connect("0.0.0.0:9999").unwrap();
    //     s.send(x).unwrap();
    //     println!("sent tcpstream");
    // });
    // thread::sleep(Duration::from_secs(1));
    // let mut x = r.try_recv().unwrap();
    // x.write(b"hi").unwrap();
    // thread::sleep(Duration::from_secs(1));
    // let mut x = r.try_recv().unwrap();
    // x.write(b"ho").unwrap();
    // thread::sleep(Duration::from_secs(100));









    // let local_socket = local_ip().unwrap();
    // let listener = TcpListener::bind(format!("{}:{}",local_socket,DEFAULT_PORT)).unwrap();

    // let mut p = ProviderDefaultV4::new();
    // let global_addr = p.get_addr().unwrap().v4addr.unwrap();
    // let global_socket = SocketAddr::new(IpAddr::V4(global_addr),DEFAULT_PORT);
    // println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);
    

    // std::thread::spawn(move || {
    //     for stream in listener.incoming() {
    //         let mut s = stream.unwrap();
    //         let mut v = vec![];
    //         s.read_to_end(&mut v).unwrap();
    //         println!("{:?}",String::from_utf8_lossy(&v));
    //     }
    // });


    // let stdin = std::io::stdin();
    // let addr = stdin.lock().lines().next().unwrap().unwrap();
    // println!("Got address");

    // match TcpStream::connect(format!("{}:{}",addr,DEFAULT_PORT)) {
    //     Ok(mut stream) => {
    //         println!("Successfully connected to server in port 3333");
    //         // click enter to send a message or attempt to contact someone
    //         for line in stdin.lock().lines() {
    //             let line = if let Ok(line) = line {
    //                 line.as_bytes().to_vec()
    //             } else {
    //                 break;
    //             };
    //             stream.write(&line).unwrap();
    //         }
    //     },
    //     Err(e) => {
    //         println!("Failed to connect: {}", e);
    //     }
    // }






    

    // let opt = Opt::from_args();
    let logger = track!(TerminalLoggerBuilder::new().destination(Destination::Stderr).level("debug".parse().unwrap()).build()).unwrap(); // info or debug

    // let mut rng = rand::thread_rng();
    // let mut n = Natpmp::new().unwrap();
    // loop {
    //     let randport_priv = rng.gen::<u16>();
    //     let randport_pub = rng.gen::<u16>();
    //     println!("{},{}",randport_priv,randport_pub);
    //     n.send_public_address_request().unwrap();
    //     n.send_port_mapping_request(Protocol::UDP, randport_priv, randport_pub, 30).unwrap();
    

    //     thread::sleep(Duration::from_millis(250));
    //     let response = n.read_response_or_retry();
    //     println!("{:?}",response);
    //     if response.is_ok() {
    //         break
    //     }
    // }

    let local_addr = "0.0.0.0".parse().unwrap();
    // let local_addr = local_ip().unwrap();
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
                println!("# SENDER: {:?}", msg.sender);

                println!("all plumtree peers: {:?}",self.node.plumtree_node().all_push_peers());
                println!("all hyparview peers: {:?} {:?}",self.node.hyparview_node().active_view(),self.node.hyparview_node().passive_view());
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