#[macro_use]
extern crate clap;
#[macro_use]
extern crate trackable;

use clap::Arg;
use fibers::sync::mpsc;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator, UnixtimeLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::net::SocketAddr;
use trackable::error::MainError;


use kora::account::*;
use curve25519_dalek::scalar::Scalar;
use std::collections::VecDeque;
use std::convert::TryInto;
use std::time::Instant;
use kora::transaction::*;
use curve25519_dalek::ristretto::{CompressedRistretto};
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use kora::randblock::*;
use kora::bloom::*;
use kora::validation::*;
use kora::constants::PEDERSEN_H;

 // cargo run --bin chat --release 6060 
fn main() -> Result<(), MainError> {
    let matches = app_from_crate!()
        .arg(Arg::with_name("PORT").index(1).required(true))
        .arg(
            Arg::with_name("CONTACT_SERVER").index(2)
                .long("contact-server")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("LOG_LEVEL")
                .long("log-level")
                .takes_value(true)
                .default_value("info")
                .possible_values(&["debug", "info"]),
        )
        .get_matches();
    let log_level = track_any_err!(matches.value_of("LOG_LEVEL").unwrap().parse())?;
    let logger = track!(TerminalLoggerBuilder::new()
        .destination(Destination::Stderr)
        .level(log_level)
        .build())?;
    let port = matches.value_of("PORT").unwrap();
    println!("port: {:?}",port);
    // let addr: SocketAddr = track_any_err!(format!("127.0.0.1:{}", port).parse())?; // ip r | grep default <--- router ip, just go to settings
    // let addr: SocketAddr = track_any_err!(format!("172.20.10.14:{}", port).parse())?;
    // let addr: SocketAddr = track_any_err!(format!("172.16.0.8:{}", port).parse())?;
    // let addr: SocketAddr = track_any_err!(format!("192.168.0.101:{}", port).parse())?;
    let addr: SocketAddr = track_any_err!(format!("172.20.10.3:{}", port).parse())?;
    // let addr: SocketAddr = track_any_err!(format!("192.168.0.1:{}", port).parse())?;
    

    let max_shards = 64usize; /* this if for testing purposes... there IS NO MAX SHARDS */
    



    let executor = track_any_err!(ThreadPoolExecutor::new())?;
    let service = ServiceBuilder::new(addr)
        .logger(logger.clone())
        .finish(executor.handle(), SerialLocalNodeIdGenerator::new()); // everyone is node 0 rn... that going to be a problem? I mean everyone has different ips...
        // .finish(executor.handle(), UnixtimeLocalNodeIdGenerator::new());
        
    let mut node = NodeBuilder::new().logger(logger).finish(service.handle());
    println!("{:?}",node.id());
    if let Some(contact) = matches.value_of("CONTACT_SERVER") {
        println!("contact: {:?}",contact);
        let contact: SocketAddr = track_any_err!(contact.parse())?;
        node.join(NodeId::new(contact, LocalNodeId::new(0)));
    }

    let l = Account::new(&format!("{}",0)).stake_acc();
    let leader0 = l.receive_ot(&l.derive_stk_ot(&Scalar::from(1u8))).unwrap(); //make a new account
    let lkey = leader0.sk.unwrap();
    let keylocation = 0;
    let staker0 = leader0.pk.compress();
    let node = LeaderNode {
        inner: node,
        key: lkey,
        keylocation: keylocation,
        stkinfo: vec![(staker0,1u64)],
        txses: vec![],
        lastblock: NextBlock::default(),
        sigs: vec![],
        queue: (0..max_shards).map(|_|[0usize;NUMBER_OF_VALIDATORS as usize].into_par_iter().collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        exitqueue: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS as usize).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        comittee: (0..max_shards).map(|_|vec![0usize;NUMBER_OF_VALIDATORS as usize]).collect::<Vec<_>>(),
        lastname: vec![],
        bnum: 0u64,
        timekeeper: Instant::now(),
    };
    executor.spawn(service.map_err(|e| panic!("{}", e)));
    executor.spawn(node);


    track_any_err!(executor.run())?;
    Ok(())
}

struct LeaderNode {
    inner: Node<Vec<u8>>,
    key: Scalar,
    keylocation: u64,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    txses: Vec<Vec<u8>>,
    lastblock: NextBlock,
    sigs: Vec<NextBlock>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bnum: u64,
    timekeeper: Instant,
}
impl Future for LeaderNode {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;

            while let Async::Ready(Some(m)) = track_try_unwrap!(self.inner.poll()) {
                let mut m = m.payload().to_vec();
                println!("# MESSAGE length: {:?}", m.len());
                let mtype = m.pop().unwrap(); // dont do unwraps that could mess up a leader
                if mtype == 0 {
                    self.txses.push(m[..10_000].to_vec());
                }
                else if mtype == 2 {
                    self.sigs.push(bincode::deserialize(&m).unwrap());
                }

                println!("pt id: {:?}",self.inner.plumtree_node().id());
                println!("pt epp: {:?}",self.inner.plumtree_node().eager_push_peers());
                println!("pt lpp: {:?}",self.inner.plumtree_node().lazy_push_peers());
                println!("hv id: {:?}",self.inner.hyparview_node().id());
                println!("hv av: {:?}",self.inner.hyparview_node().active_view());
                println!("hv pv: {:?}",self.inner.hyparview_node().passive_view());
                did_something = true;
            }
            if (self.sigs.len() >= 86) & (self.timekeeper.elapsed().as_millis() > 500) {
                let shard = 0;
                let start = Instant::now();
                self.lastblock = NextBlock::finish(&self.key, &self.keylocation, &self.sigs, &self.comittee[shard].par_iter().map(|x|*x as u64).collect::<Vec<u64>>(), &(shard as u16), &self.bnum, &self.lastname, &self.stkinfo);
                println!("time to complete shard: {:?} ms",start.elapsed().as_millis());
                let mut m = bincode::serialize(&self.lastblock).unwrap();

                let mut hasher = Sha3_512::new();
                hasher.update(&m);
                self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();

                m.push(3u8);
                self.inner.broadcast(m);
                self.lastblock.scan_as_noone(&mut self.stkinfo,&self.comittee.par_iter().map(|x|x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>());
                for i in 0..self.comittee.len() {
                    select_stakers(&self.lastname, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                }
                did_something = true;
            }
            if (self.txses.len() >= 128) | (self.bnum == 0) {
                let mut m = bincode::serialize(&self.txses).unwrap();
                m.push(1u8);
                self.inner.broadcast(m);
                self.bnum += 1;
                self.timekeeper = Instant::now();
                did_something = true;
            }
        }
        Ok(Async::NotReady)
    }
}
