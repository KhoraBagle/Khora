#[macro_use]
// #[allow(unreachable_code)]
extern crate trackable;

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use khora::network::{write_timeout, read_timeout, read_to_end_timeout};
// use khora::seal::BETA;
use parking_lot::RwLock;
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use rand::prelude::SliceRandom;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, IpAddr, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use trackable::error::MainError;
use crossbeam::channel;


use khora::{account::*, gui};
use curve25519_dalek::scalar::Scalar;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::time::{Instant, Duration};
use std::borrow::Borrow;
use khora::transaction::*;
use curve25519_dalek::ristretto::{CompressedRistretto};
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use khora::bloom::*;
use khora::validation::*;
use khora::ringmaker::*;
use khora::vechashmap::*;
use serde::{Serialize, Deserialize};
use khora::validation::{
    NUMBER_OF_VALIDATORS, SIGNING_CUTOFF, QUEUE_LENGTH, REPLACERATE, PERSON0, LONG_TERM_SHARDS,
    STAKER_BLOOM_NAME, STAKER_BLOOM_SIZE, STAKER_BLOOM_HASHES,
    READ_TIMEOUT, WRITE_TIMEOUT, NONCEYNESS,
    EXIT_TIME, USURP_TIME, DEFAULT_PORT, OUTSIDER_PORT,
    reward, blocktime, set_comittee_n, comittee_n,
};

use gip::{Provider, ProviderDefaultV4};
use colored::Colorize;



fn main() -> Result<(), MainError> {
    let logger = track!(TerminalLoggerBuilder::new().destination(Destination::Stderr).level("error".parse().unwrap()).build())?; // info or debug


    let outerlistener = TcpListener::bind(format!("0.0.0.0:{}",OUTSIDER_PORT)).unwrap();

    
    let local_socket: SocketAddr = format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap();
    // let local_socket: SocketAddr = format!("{}:{}",local_ip().unwrap(),DEFAULT_PORT).parse().unwrap();
    let mut p = ProviderDefaultV4::new();
    let global_addr = match p.get_addr() {
        Ok(x) => x.v4addr.unwrap(),
        Err(x) => {print!("Can't get global ip! Exiting because error: "); panic!("{}",x);},
    };
    // let global_addr = local_ip().unwrap(); // <----------this would be for local. uncomment above for global
    let global_socket = format!("{}:{}",global_addr,DEFAULT_PORT).parse::<SocketAddr>().unwrap();
    println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);




    // println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);
    let executor = track_any_err!(ThreadPoolExecutor::new())?;
    let service = ServiceBuilder::new(local_socket)
        .logger(logger.clone())
        .server_addr(global_socket)
        .finish(executor.handle(), SerialLocalNodeIdGenerator::new());
    let backnode = NodeBuilder::new().logger(logger.clone()).finish(service.handle());
    println!("{:?}",backnode.id());
    let frontnode = NodeBuilder::new().logger(logger).finish(service.handle()); // todo: make this local_id random so people can't guess you
    println!("{:?}",frontnode.id()); // this should be the validator survice


    let (ui_sender, urecv) = channel::unbounded::<Vec<u8>>();
    let (usend, ui_reciever) = channel::unbounded();
    let (sendtcp, recvtcp) = channel::bounded::<TcpStream>(1);

    thread::spawn(move || {
        for stream in outerlistener.incoming() {
            match stream {
                Ok(stream) => {
                    if stream.set_nonblocking(false).is_err() {
                        println!("couldn't set nonblocking");
                        continue
                    }
                    sendtcp.send(stream).unwrap();
                }
                Err(_) => {
                    println!("Error in recieving TcpStream");
                }
            }

        }
    });
    
    // the myNode file only exists if you already have an account made
    let setup = !Path::new("myNode").exists();
    if setup {
        std::thread::spawn(move || {
            let pswrd = urecv.recv().unwrap();
            let lightning_yielder = urecv.recv().unwrap()[0] == 1;

            println!("{:?}",pswrd);
            let me = Account::new(&pswrd);
            let validator = me.stake_acc().receive_ot(&me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap(); //make a new account
            let key = validator.sk.unwrap();
            if !lightning_yielder {
                NextBlock::initialize_saving();
            }
            LightningSyncBlock::initialize_saving();
            History::initialize();
    
            // everyone agrees this person starts with 1 khora token
            let mut initial_history = VecHashMap::new();
            initial_history.insert(PERSON0,(1u64,None));
            let (smine, keylocation) = {
                if initial_history.vec[0].0 == me.stake_acc().derive_stk_ot(&Scalar::from(initial_history.vec[0].1.0)).pk.compress() {
                    println!("\n\nhey i guess i founded this crypto!\n\n");
                    initial_history.vec[0].1.1 = Some(SocketAddr::new(global_socket.ip(),OUTSIDER_PORT));
                    (Some((0usize,initial_history.vec[0].1)), Some(0usize))
                } else {
                    (None, None)
                }
            };
    
            // creates the node with the specified conditions then saves it to be used from now on
            let node = KhoraNode {
                inner: NodeBuilder::new().finish( ServiceBuilder::new(format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap()).finish(ThreadPoolExecutor::new().unwrap().handle(), SerialLocalNodeIdGenerator::new()).handle()),
                outer: NodeBuilder::new().finish( ServiceBuilder::new(format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap()).finish(ThreadPoolExecutor::new().unwrap().handle(), SerialLocalNodeIdGenerator::new()).handle()),
                outerlister: channel::unbounded().1,
                lasttags: vec![],
                lastnonanony: None,
                nonanony: VecHashMap::new(),
                me,
                mine: HashMap::new(),
                reversemine: HashMap::new(),
                smine: smine.map(|x| (x.0,x.1.0)), // [location, amount]
                nmine: None,
                nheight: 0,
                key,
                keylocation,
                leader: PERSON0,
                overthrown: HashSet::new(),
                votes: vec![0;NUMBER_OF_VALIDATORS],
                stkinfo: initial_history.clone(),
                queue: (0..LONG_TERM_SHARDS).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|_| 0).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                exitqueue: (0..LONG_TERM_SHARDS).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                comittee: (0..LONG_TERM_SHARDS).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|_| 0).collect::<Vec<usize>>()).collect::<Vec<_>>(),
                lastname: Scalar::one().as_bytes().to_vec(),
                bloom: BloomFile::from_randomness(STAKER_BLOOM_NAME, STAKER_BLOOM_SIZE, STAKER_BLOOM_HASHES, true),
                bnum: 0u64,
                lastbnum: 0u64,
                height: 0u64,
                sheight: 1usize,
                alltagsever: HashSet::new(),
                txses: HashSet::new(),
                sigs: vec![],
                timekeeper: Instant::now(),
                waitingforentrybool: true,
                waitingforleaderbool: false,
                waitingforleadertime: Instant::now(),
                waitingforentrytime: Instant::now(),
                doneerly: Instant::now(),
                headshard: 0,
                usurpingtime: Instant::now(),
                is_validator: smine.is_some(),
                is_node: true,
                newest: 0usize,
                gui_sender: usend.clone(),
                gui_reciever: channel::unbounded().1,
                laststk: None,
                cumtime: 0f64,
                blocktime: blocktime(0.0),
                lightning_yielder,
                ringsize: 5,
                gui_timer: Instant::now(),
                clients: Arc::new(RwLock::new(0)),
                maxcli: 10,
                spammers: HashSet::new(),
                paniced: None,
            };
            node.save();

            let mut info = bincode::serialize(&
                vec![
                bincode::serialize(&node.me.name()).unwrap(),
                bincode::serialize(&node.me.stake_acc().name()).unwrap(),
                bincode::serialize(&node.me.nonanony_acc().name()).unwrap(),
                bincode::serialize(&node.me.sk.as_bytes().to_vec()).unwrap(),
                bincode::serialize(&node.me.vsk.as_bytes().to_vec()).unwrap(),
                bincode::serialize(&node.me.ask.as_bytes().to_vec()).unwrap(),
                ]
            ).unwrap();
            info.push(254);
            usend.send(info).unwrap();


            let node = KhoraNode::load(frontnode, backnode, usend, urecv,recvtcp);
            let mut mymoney = node.mine.iter().map(|x| node.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
            mymoney.extend(node.smine.iter().map(|x| x.1).sum::<u64>().to_le_bytes());
            mymoney.extend(node.nmine.iter().map(|x| x.1).sum::<u64>().to_le_bytes());
            mymoney.push(0);
            println!("my money: {:?}",node.smine.iter().map(|x| x.1).sum::<u64>());
            node.gui_sender.send(mymoney).unwrap(); // this is how you send info to the gui
            node.save();

            executor.spawn(service.map_err(|e| panic!("{}", e)));
            executor.spawn(node);
            println!("about to run node executer!");
            track_any_err!(executor.run()).unwrap();
        });
        // creates the setup screen (sets the values used in the loops and sets some gui options)
        let app = gui::staker::KhoraStakerGUI::new(
            ui_reciever,
            ui_sender,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            vec![],
            vec![],
            vec![],
            true,
        );
        let native_options = eframe::NativeOptions::default();
        eframe::run_native(Box::new(app), native_options);
    } else {
        let mut node = KhoraNode::load(frontnode, backnode, usend, urecv,recvtcp);
        let mut mymoney = node.mine.iter().map(|x| node.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
        mymoney.extend(node.smine.iter().map(|x| x.1).sum::<u64>().to_le_bytes());
        mymoney.extend(node.nmine.iter().map(|x| x.1).sum::<u64>().to_le_bytes());
        mymoney.push(0);
        println!("my staked money: {:?}",node.smine.iter().map(|x| x.1).sum::<u64>());
        node.gui_sender.send(mymoney).unwrap(); // this is how you send info to the gui
        println!("communication with the gui all good!");
        let app = gui::staker::KhoraStakerGUI::new(
            ui_reciever,
            ui_sender,
            node.me.name(),
            node.me.stake_acc().name(),
            node.me.nonanony_acc().name(),
            node.me.sk.as_bytes().to_vec(),
            node.me.vsk.as_bytes().to_vec(),
            node.me.ask.as_bytes().to_vec(),
            false,
        );
        let native_options = eframe::NativeOptions::default();
        std::thread::spawn(move || {
            println!("attempting sync");
            node.attempt_sync(None, true);
            executor.spawn(service.map_err(|e| panic!("{}", e)));
            executor.spawn(node);
    
            println!("running node executer");
            track_any_err!(executor.run()).unwrap();
        });
        eframe::run_native(Box::new(app), native_options);
    }
    







}

#[derive(Clone, Serialize, Deserialize, Debug)]
/// the information that you save to a file when the app is off (not including gui information like saved friends)
struct SavedNode {
    me: Account,
    mine: HashMap<u64, OTAccount>,
    reversemine: HashMap<CompressedRistretto, u64>,
    smine: Option<(usize,u64)>, // [location, amount]
    sheight: usize,
    nmine: Option<(usize,u64)>, // [location, amount]
    nheight: usize,
    key: Scalar,
    keylocation: Option<usize>,
    leader: CompressedRistretto,
    overthrown: HashSet<CompressedRistretto>,
    votes: Vec<i32>,
    stkinfo: Vec<(CompressedRistretto,(u64,Option<SocketAddr>))>,
    nonanony: Vec<(CompressedRistretto,u64)>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bloom: [u128;2],
    bnum: u64,
    lastbnum: u64,
    height: u64,
    alltagsever: HashSet<CompressedRistretto>,
    headshard: usize,
    outer_view: HashSet<NodeId>,
    outer_eager: HashSet<NodeId>,
    av: Vec<NodeId>,
    pv: Vec<NodeId>,
    cumtime: f64,
    blocktime: f64,
    lightning_yielder: bool,
    is_validator: bool,
    ringsize: u8,
    lasttags: Vec<CompressedRistretto>,
    maxcli: u8,
    lastnonanony: Option<usize>,
    laststk: Option<usize>,
    newest: usize,
    paniced: Option<(Account,Option<PolynomialTransaction>,u64)>,

}

/// the node used to run all the networking
struct KhoraNode {
    inner: Node<Vec<u8>>, // for sending and recieving messages as a validator (as in inner sanctum)
    outer: Node<Vec<u8>>, // for sending and recieving messages as a non validator (as in not inner)
    outerlister: channel::Receiver<TcpStream>, // for listening to people not in the network
    gui_sender: channel::Sender<Vec<u8>>,
    gui_reciever: channel::Receiver<Vec<u8>>,
    me: Account,
    mine: HashMap<u64, OTAccount>,
    reversemine: HashMap<CompressedRistretto, u64>,
    smine: Option<(usize,u64)>, // [location, amount]
    sheight: usize,
    nmine: Option<(usize,u64)>, // [location, amount]
    nheight: usize,
    key: Scalar,
    keylocation: Option<usize>,
    leader: CompressedRistretto, // would they ever even reach consensus on this for new people when a dishonest person is eliminated???
    overthrown: HashSet<CompressedRistretto>,
    votes: Vec<i32>,
    stkinfo: VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>,
    nonanony: VecHashMap<CompressedRistretto,u64>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bloom: BloomFile,
    bnum: u64,
    lastbnum: u64,
    height: u64,
    alltagsever: HashSet<CompressedRistretto>,
    txses: HashSet<PolynomialTransaction>,
    sigs: Vec<NextBlock>,
    timekeeper: Instant,
    waitingforentrybool: bool,
    waitingforleaderbool: bool,
    waitingforleadertime: Instant,
    waitingforentrytime: Instant,
    doneerly: Instant,
    headshard: usize,
    usurpingtime: Instant,
    is_validator: bool,
    is_node: bool,
    newest: usize,
    cumtime: f64,
    blocktime: f64,
    lightning_yielder: bool,
    gui_timer: Instant,
    ringsize: u8,
    lasttags: Vec<CompressedRistretto>,
    clients: Arc<RwLock<u8>>,
    maxcli: u8,
    spammers: HashSet<SocketAddr>,
    lastnonanony: Option<usize>,
    laststk: Option<usize>,
    paniced: Option<(Account,Option<PolynomialTransaction>,u64)>,
}

impl KhoraNode {
    /// saves the important information like staker state and block number to a file: "myNode"
    fn save(&self) {
        let sn = SavedNode {
            lasttags: self.lasttags.clone(),
            me: self.me,
            mine: self.mine.clone(),
            reversemine: self.reversemine.clone(),
            smine: self.smine.clone(), // [location, amount]
            nheight: self.nheight,
            nmine: self.nmine.clone(), // [location, amount]
            key: self.key,
            keylocation: self.keylocation.clone(),
            leader: self.leader.clone(),
            overthrown: self.overthrown.clone(),
            votes: self.votes.clone(),
            stkinfo: self.stkinfo.vec.clone(),
            queue: self.queue.clone(),
            exitqueue: self.exitqueue.clone(),
            comittee: self.comittee.clone(),
            lastname: self.lastname.clone(),
            bloom: self.bloom.get_keys(),
            bnum: self.bnum,
            lastbnum: self.lastbnum,
            height: self.height,
            sheight: self.sheight,
            alltagsever: self.alltagsever.clone(),
            headshard: self.headshard.clone(),
            outer_view: self.outer.plumtree_node().lazy_push_peers().clone(),
            outer_eager: self.outer.plumtree_node().eager_push_peers().clone(),
            paniced: self.paniced.clone(),
            laststk: self.laststk.clone(),
            cumtime: self.cumtime,
            blocktime: self.blocktime,
            lightning_yielder: self.lightning_yielder,
            is_validator: self.is_validator,
            ringsize: self.ringsize,
            av: self.outer.hyparview_node.active_view.clone(),
            pv: self.outer.hyparview_node.passive_view.clone(),
            maxcli: self.maxcli,
            nonanony: self.nonanony.vec.clone(),
            lastnonanony: self.lastnonanony,
            newest: self.newest,
        }; // just redo initial conditions on the rest
        let mut sn = bincode::serialize(&sn).unwrap();
        // println!("{:?}",sn);
        // bincode::deserialize::<SavedNode>(&sn).unwrap();
        let mut f = File::create("myNode").unwrap();
        f.write_all(&mut sn).unwrap();
    }

    /// loads the node information from a file: "myNode"
    fn load(inner: Node<Vec<u8>>, mut outer: Node<Vec<u8>>, gui_sender: channel::Sender<Vec<u8>>, gui_reciever: channel::Receiver<Vec<u8>>, outerlister: channel::Receiver<TcpStream>) -> KhoraNode {
        let mut buf = Vec::<u8>::new();
        let mut f = File::open("myNode").unwrap();
        f.read_to_end(&mut buf).unwrap();

        // println!("{:?}",buf);
        let sn = bincode::deserialize::<SavedNode>(&buf).unwrap();

        // tries to get back all the friends you may have lost since turning off the app
        sn.outer_view.iter().chain(sn.outer_eager.iter()).collect::<HashSet<_>>().into_iter().for_each(|&x| {
            outer.join(x);
        });
        outer.plumtree_node.eager_push_peers = sn.outer_eager;
        outer.plumtree_node.eager_push_peers = sn.outer_view;

        outer.hyparview_node.active_view = sn.av;
        outer.hyparview_node.passive_view = sn.pv;

        


        let key = sn.key;
        if sn.smine.is_some() {
            let message = bincode::serialize(&outer.plumtree_node().id().address().ip()).unwrap();
            let mut fullmsg = Signature::sign_message2(&key,&message);
            fullmsg.push(110);
            outer.broadcast_now(fullmsg);
        }
        
        KhoraNode {
            inner,
            outer,
            outerlister,
            gui_sender,
            gui_reciever,
            lasttags: sn.lasttags.clone(),
            timekeeper: Instant::now(),
            waitingforentrybool: true,
            waitingforleaderbool: false,
            waitingforleadertime: Instant::now(),
            waitingforentrytime: Instant::now(),
            usurpingtime: Instant::now(),
            txses: HashSet::new(), // if someone is not a leader for a really long time they'll have a wrongly long list of tx
            sigs: vec![],
            me: sn.me,
            mine: sn.mine.clone(),
            reversemine: sn.reversemine.clone(),
            smine: sn.smine.clone(), // [location, amount]
            nheight: sn.nheight,
            nmine: sn.nmine.clone(), // [location, amount]
            key: sn.key,
            keylocation: sn.keylocation.clone(),
            leader: sn.leader.clone(),
            overthrown: sn.overthrown.clone(),
            votes: sn.votes.clone(),
            queue: sn.queue.clone(),
            exitqueue: sn.exitqueue.clone(),
            comittee: sn.comittee.clone(),
            lastname: sn.lastname.clone(),
            bloom: BloomFile::from_keys(sn.bloom[0],sn.bloom[1], STAKER_BLOOM_NAME, STAKER_BLOOM_SIZE, STAKER_BLOOM_HASHES, false),
            bnum: sn.bnum,
            lastbnum: sn.lastbnum,
            height: sn.height,
            sheight: sn.sheight,
            alltagsever: sn.alltagsever.clone(),
            headshard: sn.headshard.clone(),
            is_validator: sn.is_validator,
            is_node: true,
            doneerly: Instant::now(),
            newest: sn.newest,
            paniced: sn.paniced,
            cumtime: sn.cumtime,
            blocktime: sn.blocktime,
            lightning_yielder: sn.lightning_yielder,
            gui_timer: Instant::now(),
            ringsize: sn.ringsize,
            clients: Arc::new(RwLock::new(0)),
            maxcli: sn.maxcli,
            spammers: HashSet::new(),
            nonanony: VecHashMap::from(sn.nonanony.clone()),
            stkinfo: VecHashMap::from(sn.stkinfo.clone()),
            laststk: sn.laststk,
            lastnonanony: sn.lastnonanony,
        }
    }

    /// reads a full block (by converting it to lightning then reading that)
    fn readblock(&mut self, lastblock: NextBlock, m: Vec<u8>, save: bool) -> bool {
        let lastlightning = lastblock.tolightning(&self.nonanony, &self.stkinfo);
        let l = bincode::serialize(&lastlightning).unwrap();


        let inputs = lastblock.txs.into_par_iter().map(|x| x.inputs).collect::<Vec<_>>();

        let (stkout,nonanonyout): (Vec<_>,Vec<_>) = inputs.into_par_iter().filter_map(|x| {
            if x.last() == Some(&1) {
                Some((Some(x.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap()) as usize).collect::<Vec<_>>()),None))
            } else if x.last() == Some(&2) {
                Some((None,Some(x.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap()) as usize).collect::<Vec<_>>())))
            } else {None}
        }).unzip();
        let stkout = stkout.into_par_iter().filter_map(|x|x).flatten().collect::<Vec<_>>();
        let nonanonyout = nonanonyout.into_par_iter().filter_map(|x|x).flatten().collect::<Vec<_>>();
        
        self.readlightning(lastlightning,l,Some(m), Some((stkout,nonanonyout)), save)
    }

    /// reads a lightning block and saves information when appropriate
    fn readlightning(&mut self, lastlightning: LightningSyncBlock, m: Vec<u8>, largeblock: Option<Vec<u8>>, largetx: Option<(Vec<usize>,Vec<usize>)>, save: bool) -> bool {
        let t = Instant::now();
        if lastlightning.bnum >= self.bnum {
            let v = if (lastlightning.shard as usize >= self.headshard) && (lastlightning.last_name == self.lastname) {
                lastlightning.verify_multithread(&comittee_n(lastlightning.shard as usize, &self.comittee, &self.stkinfo), &self.stkinfo).is_ok()
            } else {
                false
            };
            if v {
                println!("{}",format!("=========================================================\ngot a block at {}!",self.bnum).magenta());
                if save {
                    // saves your current information BEFORE reading the new block. It's possible a leader is trying to cause a fork which can only be determined 1 block later based on what the comittee thinks is real
                    self.save();
                }

                if let Some(k) = &self.keylocation {
                    if lastlightning.info.stkout.contains(k) {
                        self.gui_sender.send(vec![6]).unwrap();
                    }
                }

                // if you are one of the validators who leave this turn, it is your responcibility to send the block to the outside world
                if let Some(keylocation) = self.keylocation {
                    let c = comittee_n(self.headshard,&self.comittee, &self.stkinfo);
                    if self.exitqueue[self.headshard].par_iter().take(REPLACERATE).map(|&x| c[x]).any(|x| keylocation == x) {
                        if let Some(mut lastblock) = largeblock.clone() {
                            lastblock.push(3);
                            println!("{}",format!("sending block {} to the outside world!",lastlightning.bnum).green());
                            self.outer.broadcast_now(lastblock); /* broadcast the block to the outside world */
                        }
                    }
                }
                self.headshard = lastlightning.shard as usize;


                self.overthrown.remove(&self.stkinfo.vec[lastlightning.leader.pk as usize].0);
                if self.stkinfo.vec[lastlightning.leader.pk as usize].0 != self.leader {
                    self.overthrown.insert(self.leader);
                }

                // if you're synicng, you just infer the empty blocks that no one saves
                for _ in self.bnum..lastlightning.bnum {
                    // println!("I missed a block!");
                    let reward = reward(self.cumtime,self.blocktime);
                    self.cumtime += self.blocktime;
                    self.blocktime = blocktime(self.cumtime);

                    NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut self.smine, reward);
                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);

                    if !self.lightning_yielder {
                        NextBlock::save(&vec![]);
                    }
                    LightningSyncBlock::save(&vec![]);





                    self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                    for i in 0..self.comittee.len() {
                        select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                    }
                    self.bnum += 1;
                }


                // println!("none time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());
                // calculate the reward for this block as a function of the current time and scan either the block or an empty block based on conditions
                let reward = reward(self.cumtime,self.blocktime);
                if !lastlightning.info.is_empty() {



                    if let Some(x) = largetx {
                        if let Some(y) = self.laststk {
                            if x.0.contains(&y) {
                                self.gui_sender.send(vec![6]).unwrap();
                                self.laststk = None;
                            } else if self.is_validator || lastlightning.info.stkin.par_iter().any(|x| x.0 == y) {
                                self.laststk = None;
                                self.gui_sender.send(vec![11]).unwrap();
                            }
                        }
                        if let Some(y) = self.lastnonanony {
                            if x.1.contains(&y) {
                                self.gui_sender.send(vec![9]).unwrap();
                                self.lastnonanony = None;
                            } else if lastlightning.info.nonanonyin.par_iter().any(|x| x.0 == y) {
                                self.lastnonanony = None;
                                self.gui_sender.send(vec![10]).unwrap();
                            }
                        }
                    } else {
                        if let Some(y) = self.laststk {
                            if let Some((_,a)) = self.smine {
                                if let Some(x) = lastlightning.info.stkin.par_iter().find_first(|x|x.0 == y) {
                                    if x.1 > a {
                                        self.laststk = None;
                                        self.gui_sender.send(vec![11]).unwrap();
                                    } else {
                                        self.laststk = None;
                                        self.gui_sender.send(vec![6]).unwrap();

                                    }
                                }
                            } else {
                                self.laststk = None;
                                self.gui_sender.send(vec![11]).unwrap();
                            }
                        }
                        if let Some(y) = self.lastnonanony {
                            if let Some((_,a)) = self.nmine {
                                if let Some(x) = lastlightning.info.nonanonyin.par_iter().find_first(|x|x.0 == y) {
                                    if x.1 > a {
                                        self.lastnonanony = None;
                                        self.gui_sender.send(vec![10]).unwrap();
                                    } else {
                                        self.lastnonanony = None;
                                        self.gui_sender.send(vec![9]).unwrap();

                                    }
                                }
                            } else {
                                self.lastnonanony = None;
                                self.gui_sender.send(vec![10]).unwrap();
                            }
                        }
                    }
                    if let Some(x) = self.lastnonanony.borrow() {
                        if self.bnum%NONCEYNESS == 0 {
                            self.lastnonanony = None;
                            self.gui_sender.send(vec![10]).unwrap();
                        } else {
                            self.lastnonanony = follow(*x,&lastlightning.info.nonanonyout,self.nheight);
                        }
                    }
                    if let Some(x) = self.laststk.borrow() {
                        if self.bnum%NONCEYNESS == 0 {
                            self.laststk = None;
                            self.gui_sender.send(vec![11]).unwrap();
                        } else {
                            self.laststk = follow(*x,&lastlightning.info.stkout,self.sheight);
                        }
                    }
                    if self.lasttags.par_iter().any(|x| {
                        lastlightning.info.tags.contains(&x)
                    }) {
                        self.lasttags = vec![];
                        self.gui_sender.send(vec![5]).unwrap();
                    }


                    if lastlightning.scanstk(&self.me, &mut self.smine, true, &mut self.sheight, &self.comittee, reward, &self.stkinfo) {
                        let message = bincode::serialize(&self.outer.plumtree_node().id().address().ip()).unwrap();
                        let mut fullmsg = Signature::sign_message2(&self.key,&message);
                        fullmsg.push(110);
                        self.outer.broadcast_now(fullmsg);
                    }
                    lastlightning.scannonanony(&self.me, &mut self.nmine, &mut self.nheight);
                    lastlightning.scan(&self.me, &mut self.mine, &mut self.reversemine, &mut self.height, &mut self.alltagsever);

                    

                    // println!("saving block...");
                    lastlightning.update_bloom(&mut self.bloom,false);
                    if !self.lightning_yielder {
                        NextBlock::save(&largeblock.unwrap()); // important! if you select to recieve full blocks you CAN NOT recieve with lightning blocks (because if you do youd miss full blocks)
                    }
                    LightningSyncBlock::save(&m);

                    if let Some(x) = &self.smine {
                        self.keylocation = Some(x.0)
                    }
                    lastlightning.scan_as_noone(&mut self.stkinfo,&mut self.nonanony, &mut self.queue, &mut self.exitqueue, &mut self.comittee, reward, true);

                    self.lastbnum = self.bnum;
                    let mut hasher = Sha3_512::new();
                    hasher.update(m);
                    self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                } else {



                    if self.bnum%NONCEYNESS == 0 && self.lastnonanony.is_some() {
                        self.lastnonanony = None;
                        self.gui_sender.send(vec![10]).unwrap();
                    }
                    if self.laststk.is_some() && self.is_validator {
                        self.laststk = None;
                        self.gui_sender.send(vec![11]).unwrap();
                    }

                    NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut self.smine, reward);
                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);
                    if !self.lightning_yielder {
                        NextBlock::save(&vec![]);
                    }
                    LightningSyncBlock::save(&vec![]);
                }
                // println!("some time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());

                self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                self.newest = self.queue[self.headshard][0];
                for i in 0..self.comittee.len() {
                    select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                }
                self.bnum += 1;

                
                self.votes = self.votes.par_iter().zip(comittee_n(self.headshard,&self.comittee, &self.stkinfo).par_iter()).map(|(z,&x)| z + lastlightning.validators.iter().filter(|y| y.pk == x).count() as i32).collect::<Vec<_>>();
                


                

                // println!("midd time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());
                
                let s = &self.stkinfo.vec;
                let o = &self.overthrown;
                let cm = &comittee_n(self.headshard,&self.comittee, &self.stkinfo);
                /* LEADER CHOSEN BY VOTES (off blockchain, says which comittee member they should send stuff to) */
                let abouttoleave = self.exitqueue[self.headshard].par_iter().take(EXIT_TIME).map(|z| cm[*z].clone()).collect::<HashSet<_>>();
                self.leader = self.stkinfo.vec[*comittee_n(self.headshard,&self.comittee, &self.stkinfo).par_iter().zip(self.votes.par_iter()).max_by_key(|(x,&y)| {
                    if abouttoleave.contains(x) || o.contains(&s[**x].0) {
                        i32::MIN
                    } else {
                        y
                    }
                }).unwrap().0].0;
                /* LEADER CHOSEN BY VOTES (off blockchain, says which comittee member they should send stuff to) */

                
                // send info to the gui
                let mut mymoney = self.mine.par_iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
                mymoney.extend(self.smine.unwrap_or_default().1.to_le_bytes());
                mymoney.extend(self.nmine.unwrap_or_default().1.to_le_bytes());
                mymoney.push(0);
                self.gui_sender.send(mymoney).unwrap(); // this is how you send info to the gui

                let mut thisbnum = self.bnum.to_le_bytes().to_vec();
                thisbnum.push(2);
                self.gui_sender.send(thisbnum).unwrap(); // this is how you send info to the gui

                // println!("block {} name: {:?}",self.bnum, self.lastname);

                // delete the set of overthrone leaders sometimes to give them another chance
                if self.bnum % 128 == 0 {
                    self.overthrown = HashSet::new();
                }
                // if you save the history, the txses you know about matter; otherwise, they don't (becuase you're not involved in block creation)
                let s = self.stkinfo.borrow();
                let n = self.nonanony.borrow();
                let b = self.bnum/NONCEYNESS;
                let bloom = self.bloom.borrow();
                println!("{}",format!("had {} tx",self.txses.len()).magenta());
                println!("{}",format!("block had {} stkin, {} stkout, {} otain",lastlightning.info.stkin.len(),lastlightning.info.stkin.len(),lastlightning.info.txout.len()).magenta());
                // println!("late time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());
                self.txses = self.txses.par_iter().filter(|t| {
                    if t.inputs.last() == Some(&0u8) {
                        t.tags.par_iter().all(|y| !bloom.contains(y.as_bytes()))
                    } else if t.inputs.last() == Some(&1u8) {
                        t.verifystk(s,b).is_ok()
                    } else if t.inputs.last() == Some(&2u8) {
                        t.verifynonanony(n,b).is_ok()
                    } else {
                        false
                    }
                }).cloned().collect();
                // println!("most time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());

                println!("{}",format!("have {} tx",self.txses.len()).magenta());
                // runs any operations needed for the panic button to function
                self.send_panic_or_stop(&lastlightning, save);

                // if you're lonely and on the comittee, you try to reconnect with the comittee (WARNING: DOES NOT HANDLE IF YOU HAVE FRIENDS BUT THEY ARE IGNORING YOU)
                if self.is_validator && self.inner.plumtree_node().all_push_peers().is_empty() {
                    for n in comittee_n(self.headshard,&self.comittee, &self.stkinfo).iter().filter_map(|&x| self.stkinfo.vec[x].1.1).collect::<Vec<_>>() {
                        self.inner.join(NodeId::new(SocketAddr::from((n.ip(),DEFAULT_PORT)),LocalNodeId::new(1)));
                    }
                }



                self.cumtime += self.blocktime;
                self.blocktime = blocktime(self.cumtime);

                self.gui_sender.send(vec![self.blocktime as u8,128]).unwrap();

                self.sigs = vec![];
                set_comittee_n(lastlightning.shard as usize, &self.comittee, &self.stkinfo);
                self.doneerly = self.timekeeper;
                self.waitingforentrybool = true;
                self.waitingforleaderbool = false;
                self.waitingforleadertime = Instant::now();
                self.waitingforentrytime = Instant::now();
                self.timekeeper = Instant::now();
                self.headshard = 0;
                self.usurpingtime = Instant::now();
                // println!("block reading process done!!!");










                // this section tells you if you're on the comittee or not
                // if you're on the comittee you need to pull on your inner node
                // if you're not you need to poll on your user node
                self.is_validator = false;
                if let Some(keylocation) = self.keylocation {
                    let keylocation = keylocation as usize;
                    self.is_validator = comittee_n(self.headshard,&self.comittee, &self.stkinfo).par_iter().any(|x| *x == keylocation);
                    self.gui_sender.send(vec![self.is_validator as u8,3]).unwrap();

                    if self.queue[self.headshard].par_iter().take(REPLACERATE).any(|&x| x == keylocation) {
                        // announce yourself to the comittee because it's about to be your turn
                        let si = &self.stkinfo;
                        for n in comittee_n(self.headshard,&self.comittee, &self.stkinfo).par_iter().filter_map(|&x| si.get_by_index(x).1.1).collect::<Vec<_>>() {
                            self.inner.join(NodeId::new(SocketAddr::from((n.ip(),DEFAULT_PORT)),LocalNodeId::new(1)));
                        }
                    }
                }
                self.is_node = !self.is_validator;

                println!("nonanony info: {:?}",self.nonanony.vec);
                println!("stake info: {:?}",self.stkinfo.vec);
                println!("full time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());
                println!("known validator's ips: {:?}", self.stkinfo.vec.iter().filter_map(|(_,(_,x))| x.clone()).collect::<Vec<_>>());
                return true
            }
        }
        false
    }

    /// runs the operations needed for the panic button to work
    fn send_panic_or_stop(&mut self, lastlightning: &LightningSyncBlock, send: bool) {
        let mut done = true;
        if let Some((a,t,fee)) = &mut self.paniced {
            if let Some(b) = &t {
                if lastlightning.info.tags.contains(&b.tags[0]) {
                    *t = None;
                } else {
                    done = false;
                    let mut txbin = bincode::serialize(&b).unwrap();
                    self.txses.insert(b.clone());
                    txbin.push(0);
                    if send {
                        self.outer.broadcast_now(txbin);
                    }
                }
            }


            let s = a.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
            if let Some(x) = self.stkinfo.get_index_by_key(&s) {
                let smine: (usize, u64) = (x,self.stkinfo.vec[x].1.0);


                let (loc, amnt) = smine.clone();
                let inps = a.receive_ot(&a.derive_stk_ot(&Scalar::from(amnt))).unwrap();

                let fee = if *fee < amnt {*fee} else {0};
                let mut outs = vec![];
                outs.push((&self.me,Scalar::from(amnt - fee)));

                
                let tx = Transaction::spend_ring_nonce(&vec![inps], &outs.iter().map(|x| (x.0,&x.1)).collect(),self.bnum/NONCEYNESS);
                // println!("about to verify!");
                // tx.verify().unwrap();
                // println!("finished to verify!");
                let mut loc = loc.to_le_bytes().to_vec();
                loc.push(1);
                let tx = tx.polyform(&loc); // push 0
                if tx.verifystk(&self.stkinfo,self.bnum/NONCEYNESS).is_ok() {
                    done = false;
                    let mut txbin = bincode::serialize(&tx).unwrap();
                    self.txses.insert(tx);
                    txbin.push(0);
                    self.outer.broadcast_now(txbin.clone());
                    println!("sending tx!");
                } else {
                    println!("you can't make that transaction!");
                }
            };

            let s = a.nonanony_acc().derive_stk_ot(&Scalar::one()).pk.compress();
            if let Some(x) = self.nonanony.get_index_by_key(&s) {
                let smine: (usize, u64) = (x,self.nonanony.vec[x].1);


                let (loc, amnt) = smine.clone();
                let inps = a.receive_ot(&a.derive_stk_ot(&Scalar::from(amnt))).unwrap();

                let fee = if *fee < amnt {*fee} else {0};
                let mut outs = vec![];
                outs.push((&self.me,Scalar::from(amnt - fee)));

                
                let tx = Transaction::spend_ring_nonce(&vec![inps], &outs.iter().map(|x| (x.0,&x.1)).collect(),self.bnum/NONCEYNESS);
                // println!("about to verify!");
                // tx.verify().unwrap();
                // println!("finished to verify!");
                let mut loc = loc.to_le_bytes().to_vec();
                loc.push(1);
                let tx = tx.polyform(&loc); // push 0
                if tx.verifynonanony(&self.nonanony,self.bnum/NONCEYNESS).is_ok() {
                    done = false;
                    let mut txbin = bincode::serialize(&tx).unwrap();
                    self.txses.insert(tx);
                    txbin.push(0);
                    self.outer.broadcast_now(txbin.clone());
                    println!("sending tx!");
                } else {
                    println!("you can't make that transaction!");
                }
            };
        }
        if done {
            self.paniced = None;
        }
    }



    /// returns the responces of each person you sent it to and deletes those who are dead from the view
    fn attempt_sync(&mut self, node: Option<SocketAddr>, entering: bool) {

        println!("attempting sync");
        let mut rng = &mut rand::thread_rng();
        let mut sendview = if let Some(x) = node {
            vec![x]
        } else if let Some(x) = self.stkinfo.hashmap.get(&self.me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress()) {
            let mut y = self.stkinfo.vec.clone();
            let yend = y.len()-1;
            y.swap(yend, *x);
            y.pop();
            y.iter().filter_map(|(_,(_,x))| *x).collect::<Vec<_>>()
        } else {
            self.stkinfo.vec.iter().filter_map(|(_,(_,x))| *x).collect::<Vec<_>>()
        };

        if sendview.len() == 0 {
            println!("your sendview is empty");
        }
        sendview.shuffle(&mut rng);
        for node in sendview {
            println!("searching for node...");
            if let Ok(mut stream) = TcpStream::connect_timeout(&node,CONNECT_TIMEOUT) {
                if stream.set_nonblocking(false).is_err() {
                    println!("couldn't set nonblocking");
                    continue
                }
                println!("connected...");
                let mut bnum = self.bnum.to_le_bytes().to_vec();
                bnum.push(122 - (self.lightning_yielder as u8));
                // if stream.write(&bnum).is_ok() {
                if write_timeout(&mut stream, &bnum, WRITE_TIMEOUT) {
                    println!("request made...");
                    let mut ok = [0;8];
                    if read_timeout(&mut stream, &mut ok, READ_TIMEOUT) {
                        let syncnum = u64::from_le_bytes(ok);
                        self.gui_sender.send(ok.iter().chain(&[7u8]).cloned().collect()).unwrap();
                        println!("responce valid. syncing now...");
                        let mut blocksize = [0u8;8];
                        let mut counter = 0;
                        // while read_timeout(&mut stream,&mut blocksize,READ_TIMEOUT) {
                        while stream.read_exact(&mut blocksize).is_ok() {
                            counter += 1;
                            let bsize = u64::from_le_bytes(blocksize) as usize;
                            println!("block: {} of {} -- {} bytes",self.bnum,syncnum,bsize);
                            let mut serialized_block = vec![0u8;bsize];
                            if !read_timeout(&mut stream,&mut serialized_block,READ_TIMEOUT) {
                                println!("couldn't read the block");
                                break
                            };
                            if self.lightning_yielder {
                                if let Ok(lastblock) = bincode::deserialize::<LightningSyncBlock>(&serialized_block) {
                                    if !self.readlightning(lastblock, serialized_block, None, None, (counter%1000 == 0) || (self.bnum >= syncnum-1)) {
                                        break
                                    }
                                } else {
                                    break
                                }
                            } else {
                                if let Ok(lastblock) = bincode::deserialize::<NextBlock>(&serialized_block) {
                                    if !self.readblock(lastblock, serialized_block, (counter%1000 == 0) || (self.bnum >= syncnum-1)) {
                                        break
                                    }
                                } else {
                                    break
                                }
                            }
                        }
                        break
                    } else {
                        println!("They didn't respond!");
                    }
                } else {
                    println!("can't write to stream!");
                }
            } else {
                println!("this friend is probably busy or you have none. you can't connect to them.");
            }
        }
        self.gui_sender.send([0u8;8].iter().chain(&[7u8]).cloned().collect()).unwrap();
        println!("done syncing!");
    }

    /// returns the responces of each person you sent it to and deletes those who are dead from the view
    fn get_tx(&mut self) -> (Vec<u8>,usize) {

        if self.txses.len() == 0 {
            let mut rng = &mut rand::thread_rng();
            let mut sendview = {
                let mut y = self.stkinfo.vec.clone();
                let yend = y.len()-1;
                y.swap(yend, self.stkinfo.hashmap[&self.me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress()]);
                y.pop();
                y.iter().filter_map(|(_,(_,x))| *x).collect::<Vec<_>>()
            };
            
            sendview.shuffle(&mut rng);
            for node in sendview {
                if let Ok(mut stream) =  TcpStream::connect_timeout(&node, CONNECT_TIMEOUT) {
                    if stream.set_nonblocking(false).is_err() {
                        println!("couldn't set nonblocking");
                        continue
                    }
                    if stream.write(&[1]).is_ok() {
                    // if write_timeout(&mut stream, &[1], WRITE_TIMEOUT) {
                        if let Some(txses) = read_to_end_timeout(&mut stream, READ_TIMEOUT) {
                            if let Ok(x) = bincode::deserialize::<Vec<PolynomialTransaction>>(&txses) {
                                // x.retain(|t|
                                //     {
                                //         if t.inputs.last() == Some(&1) {
                                //             t.verifystk(&self.stkinfo,self.bnum/NONCEYNESS).is_ok()
                                //         } else if t.inputs.last() == Some(&2) {
                                //             t.verifynonanony(&self.nonanony,self.bnum/NONCEYNESS).is_ok()
                                //         } else {
                                //             let bloom = self.bloom.borrow();
                                //             t.tags.iter().all(|y| !bloom.contains(y.as_bytes())) && t.verify().is_ok()
                                //         }
                                //     }
                                // );
                                // if x.len() > self.txses.len() {
                                //     return (txses,x.len())
                                // }
                                if x.len() > self.txses.len() {
                                    return (txses,x.len())
                                }
                            }
                        } else {
                            println!("can't read to end!");
                        }
                    } else {
                        println!("can't write to stream!");
                    }
                } else {
                    println!("your friend is probably busy or you have none");
                }
            }
        }

        (bincode::serialize(&self.txses.par_iter().collect::<Vec<_>>()).unwrap(),self.txses.len())
    }



}
impl Future for KhoraNode {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;

            /*\_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner|\
            \*/
            /*\control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________|--\
            \*/
            /*\_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner|----\
            \*/
            /*\control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________|----/
            \*/
            /*\_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner|--/
            \*/
            /*\control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________|/
            \*/
            if self.is_validator {
                // done early tells you if you need to wait before starting the next block stuff because you finished the last block early
                if ((self.doneerly.elapsed().as_secs() > self.blocktime as u64) && (self.doneerly.elapsed() >= self.timekeeper.elapsed())) || self.timekeeper.elapsed().as_secs() > self.blocktime as u64 {
                    self.waitingforentrybool = true;
                    self.waitingforleaderbool = false;
                    self.waitingforleadertime = Instant::now();
                    self.waitingforentrytime = Instant::now();
                    self.timekeeper = Instant::now();
                    self.doneerly = Instant::now();
                    println!("{}","ready to make block".red());
    
                    // if you are the newest member of the comittee you're responcible for choosing the tx that goes into the next block
                    if self.keylocation == Some(self.newest) {
                        let (m,l) = self.get_tx();
                        println!("{}",format!("sending {} tx as newest",l).red().bold());

                        let mut m = Signature::sign_message_nonced(&self.key, &m, &self.newest,&self.bnum);
                        m.push(1u8);
                        self.inner.broadcast(m);
                    }
                }
            }
            // if you need to usurp shard 0 because of huge network failure
            if self.usurpingtime.elapsed().as_secs() > USURP_TIME {
                self.timekeeper = self.usurpingtime;
                self.usurpingtime = Instant::now();
                self.headshard += 1;
                println!("{}","SOMETHING WENT VERY WRONG WITH THE COMITTEE. SHARD IS BEING USURPED.".red().bold());

            }


            // updates some gui info
            if self.gui_timer.elapsed().as_secs() > 5 {
                self.gui_timer = Instant::now();
                let mut friend = self.outer.plumtree_node().all_push_peers();
                friend.remove(self.outer.plumtree_node().id());
                let friend = friend.into_iter().collect::<Vec<_>>();
                // println!("friends: {:?}",friend);
                let mut gm = (friend.len() as u16).to_le_bytes().to_vec();
                gm.push(4);
                self.gui_sender.send(gm).unwrap();
            }






             /*\__________________________________________________________________________________________________________________________
        |--0| VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::\
        |--0| ::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF|\
        |--0| VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::|/\
        |--0| ::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF/\/\___________________________________
        |--0| VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::\/\/
        |--0| ::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF|\/
        |--0| VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::|/
        |--0| ::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF/
             \*/
            // this section is for if you're on the comittee (or are very soon about to be)
            if self.is_validator {
                while let Async::Ready(Some(fullmsg)) = track_try_unwrap!(self.inner.poll()) {
                    let msg = fullmsg.message.clone();
                    let mut m = msg.payload.to_vec();
                    if let Some(mtype) = m.pop() {
                        if mtype == 2 {
                            print!("{}",".".red());
                        }
                        else {
                            println!("{}",format!("recieved message type {:?} from {:?}", mtype,msg.id.node()).red());
                        }


                        if mtype == 1 /* the transactions you're supposed to filter and make a block for */ {
                            if let Some(who) = Signature::recieve_signed_message_nonced(&mut m, &self.stkinfo, &self.bnum) {
                                if (who == self.newest) || (self.stkinfo.vec[who as usize].0 == self.leader) {
                                    if let Ok(m) = bincode::deserialize::<Vec<PolynomialTransaction>>(&m) {

                                        if let Some(keylocation) = &self.keylocation {
                                            let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, m, &(self.headshard as u8), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo, &self.nonanony);
                                            let mut m = bincode::serialize(&m).unwrap();
                                            m.push(2);
                                            for _ in comittee_n(self.headshard,&self.comittee, &self.stkinfo).iter().filter(|&x|x == keylocation).collect::<Vec<_>>() {
                                                // println!("{}","broadcasting block signature".red());
                                                self.inner.broadcast(m.clone());
                                            }
                                        }
                                        self.waitingforentrybool = false;
                                        self.waitingforleaderbool = true;
                                        self.waitingforleadertime = Instant::now();
                                        self.inner.handle_gossip_now(fullmsg, true);

                                        continue
                                    }
                                }
                            }
                        } else if mtype == 2 /* the signatures you're supposed to process as the leader */ {
                            if let Ok(sig) = bincode::deserialize(&m) {
                                // println!("{}","recieved block signature".red());
                                self.sigs.push(sig);
                                self.inner.handle_gossip_now(fullmsg, true);

                                continue
                            }
                        } else if mtype == 3 /* the full block that was made */ {
                            if let Ok(lastblock) = bincode::deserialize::<NextBlock>(&m) {
                                if self.readblock(lastblock, m, true) {
                                    println!("{}","recieved full block".red());
                                    self.inner.handle_gossip_now(fullmsg, true);

                                    continue;
                                }
                            }
                        }
                        /* spam that you choose not to propegate */ 
                        println!("{}","a validator sent spam".bright_yellow().bold());
                        self.inner.kill(&fullmsg.sender);
                        self.inner.handle_gossip_now(fullmsg, false);
                        
                    }
                    did_something = true;
                }
                // if the leader didn't show up, overthrow them
                if (self.waitingforleadertime.elapsed().as_secs() > (0.5*self.blocktime) as u64) && self.waitingforleaderbool {
                    println!("{}","OVERTHROWING LEADER".red().bold());
                    self.waitingforleadertime = Instant::now();
                    /* change the leader, also add something about only changing the leader if block is free */

                    self.overthrown.insert(self.leader);
                    self.leader = self.stkinfo.vec[*comittee_n(self.headshard,&self.comittee, &self.stkinfo).iter().zip(self.votes.iter()).max_by_key(|(&x,&y)| { // i think it does make sense to not care about whose going to leave soon here
                        if self.overthrown.contains(&self.stkinfo.vec[x].0) {
                            i32::MIN
                        } else {
                            y
                        }
                    }).unwrap().0].0;
                }
                /*_________________________________________________________________________________________________________
                LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||||
                ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF|
                LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||||
                ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF|
                LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||||
                ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF|
                LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||||
                ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF|
                *//////////////////////////////////////////////////////////////////////////////////////////////////////////
                // if you are the leader, run these block creation commands
                if self.me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress() == self.leader {
                    if (self.sigs.len() > SIGNING_CUTOFF) && (self.timekeeper.elapsed().as_secs() > (0.25*self.blocktime) as u64) {
                        if let Ok(lastblock) = NextBlock::finish(&self.key, &self.keylocation.iter().next().unwrap(), &self.sigs, &comittee_n(self.headshard,&self.comittee, &self.stkinfo), &(self.headshard as u8), &self.bnum, &self.lastname, &self.stkinfo,&self.nonanony) {
                            
                            lastblock.verify(&comittee_n(self.headshard,&self.comittee, &self.stkinfo), &self.stkinfo,&self.nonanony).unwrap();
    
                            let mut m = bincode::serialize(&lastblock).unwrap();
                            m.push(3u8);
                            self.inner.broadcast(m);
        
        
                            self.sigs = vec![];
        
                            
                            println!("{}",format!("made a block with {} transactions!",lastblock.txs.len()).red());

                            did_something = true;

                        } else {
                            let comittee = &comittee_n(self.headshard,&self.comittee, &self.stkinfo);
                            self.sigs.retain(|x| !comittee.into_par_iter().all(|y| x.leader.pk != *y));
                            let a = self.leader.to_bytes().to_vec();
                            let b = vec![self.headshard as u8];
                            let c = self.bnum.to_le_bytes().to_vec();
                            let d = self.lastname.clone();
                            let e = &self.stkinfo;
                            let f = &self.nonanony;
                            self.sigs.retain(|x| {
                                let m = vec![a.clone(),b.clone(),Syncedtx::to_sign(&x.txs,f,e),c.clone(),d.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
                                let mut s = Sha3_512::new();
                                s.update(&m);
                                Signature::verify(&x.leader, &mut s.clone(),&e)
                            });

                            println!("{}","FAILED TO MAKE A BLOCK RIGHT NOW".red().bold());
                        }
                    }
                }

                // if the entry person didn't show up start trying to make an empty block
                if self.waitingforentrybool && (self.waitingforentrytime.elapsed().as_secs() > (0.66*self.blocktime) as u64) {
                    self.waitingforentrybool = false;
                    for keylocation in &self.keylocation {
                        let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, vec![], &(self.headshard as u8), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo, &self.nonanony);
                        println!("{}","ATTEMPTING TO MAKE AN EMPTY BLOCK (this does nothing if the block was already made)".red());
                        let mut m = bincode::serialize(&m).unwrap();
                        m.push(2);
                        if comittee_n(self.headshard,&self.comittee, &self.stkinfo).contains(&(*keylocation as usize)) {
                            self.inner.broadcast(m);
                        }
                    }
                }
            }
                /*\______________________________________________________________________________________________
            |--0| STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::\
            |--0| ::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF|\
            |--0| STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::|/\
            |--0| ::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF/\/\___________________________________
            |--0| STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::\/\/
            |--0| ::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF|\/
            |--0| STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::|/
            |--0| ::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF/
                \*/
            // if you're not in the comittee
            // if self.is_node {

                // jhgfjhfgj
                while let Ok(mut stream) = self.outerlister.try_recv() {
                    println!("{}","got a stream!".blue());
                    if let Ok(sa) = stream.peer_addr() {
                        if !self.spammers.contains(&sa) {
                            let mut m = vec![0;100_000]; // maybe choose an upper bound in an actually thoughtful way?
                            if let Ok(i) = stream.read(&mut m) { // stream must be read before responding btw
                                m.truncate(i);
                                if let Some(mtype) = m.pop() {
                                    println!("{}",format!("got a stream of type {}!",mtype).blue());
                                    if mtype == 0 /* transaction someone wants to make */ {
                                        if let Ok(t) = bincode::deserialize::<PolynomialTransaction>(&m) {
                                            let ok = {
                                                if t.inputs.last() == Some(&0u8) {
                                                    let bloom = self.bloom.borrow();
                                                    t.tags.iter().all(|y| !bloom.contains(y.as_bytes())) && t.verify().is_ok()
                                                } else if t.inputs.last() == Some(&1u8) {
                                                    t.verifystk(&self.stkinfo,self.bnum/NONCEYNESS).is_ok()
                                                } else if t.inputs.last() == Some(&2u8) {
                                                    t.verifynonanony(&self.nonanony,self.bnum/NONCEYNESS).is_ok()
                                                } else {
                                                    false
                                                }
                                            };
                                            if ok {
                                                println!("{}","a user sent a tx".blue());
                                                m.push(0);
                                                self.outer.broadcast_now(m);
                                                stream.write(&[1u8]).is_ok();
                                                // write_timeout(&mut stream, &[1u8], WRITE_TIMEOUT);
                                                stream.flush();
                                            }
                                        }
                                    } else if mtype == 1 {
                                        println!("{}","a validator needs some tx".blue());
                                        stream.write(&bincode::serialize(&self.txses.iter().collect::<Vec<_>>()).unwrap()).is_ok();
                                        // write_timeout(&mut stream, &bincode::serialize(&self.txses.iter().collect::<Vec<_>>()).unwrap(), WRITE_TIMEOUT);
                                        stream.flush();
                                    } else if mtype == 101 /* e */ {
                                        println!("{}","someone wants a bunch of ips".blue());
                                        let x = self.stkinfo.vec.iter().filter_map(|(_,(_,x))| *x).collect::<Vec<_>>();
                                        stream.write(&bincode::serialize(&x).unwrap()).is_ok();
                                        // write_timeout(&mut stream, &bincode::serialize(&x).unwrap(), WRITE_TIMEOUT);
                                        stream.flush();
                                    } else if mtype == 114 /* r */ { // answer their ring question
                                        if let Ok(r) = recieve_ring(&m) {
                                            println!("{}","someone wants a ring".blue());
                                            thread::spawn(move || {
                                                for i in r {
                                                    if !write_timeout(&mut stream, &History::get_raw(&i), WRITE_TIMEOUT) {
                                                        break
                                                    }
                                                }
                                                stream.flush();
                                            });
                                        }
                                    } else if mtype == 121 /* y */ { // someone sent a sync request
                                        println!("{}","someone wants lightning".blue());
                                        if *self.clients.read() < self.maxcli {
                                            println!("{}","helping client".blue());
                                            let bnum = self.bnum;
                                            *self.clients.write() += 1;
                                            let cli = self.clients.clone();
                                            if let Ok(m) = m.try_into() {
                                                let mut sync_theirnum = u64::from_le_bytes(m) + 1;
                                                // let mut sync_theirnum = u64::from_le_bytes(m);
                                                println!("{}",format!("syncing them from {}",sync_theirnum).blue());
                                                thread::spawn(move || {
                                                    if stream.write(&bnum.to_le_bytes()).is_ok() {
                                                    // write_timeout(&mut stream, &bnum.to_le_bytes(), WRITE_TIMEOUT) {
                                                        loop {
                                                            match LightningSyncBlock::read(&sync_theirnum) {
                                                                Ok(x) => {
                                                                    println!("{}: {} bytes",sync_theirnum,x.len());
                                                                    if !stream.write(&(x.len() as u64).to_le_bytes()).is_ok() || !stream.write(&x).is_ok() {println!("Couldn't write block");break}
                                                                    // if !write_timeout(&mut stream, &(x.len() as u64).to_le_bytes(), WRITE_TIMEOUT) {
                                                                    //     break
                                                                    // }
                                                                    // if !write_timeout(&mut stream, &x, WRITE_TIMEOUT) {
                                                                    //     break
                                                                    // }
                                                                },
                                                                Err(x) => ()//println!("Err: {}",x)
                                                            }
                                                            if sync_theirnum == bnum {
                                                                break
                                                            }
                                                            sync_theirnum += 1;
                                                        }
                                                    }
                                                    stream.flush();
                                                });
                                            }
                                            *cli.write() -= 1;
                                        }
                                    } else if mtype == 122 /* z */ { // someone sent a full sync request
                                        println!("{}","someone wants full blocks".blue());
                                        if !self.lightning_yielder && *self.clients.read() < self.maxcli {
                                            let bnum = self.bnum;
                                            *self.clients.write() += 1;
                                            let cli = self.clients.clone();
                                            thread::spawn(move || {
                                                if let Ok(m) = m.try_into() {
                                                    if stream.write(&bnum.to_le_bytes()).is_ok() {
                                                    // write_timeout(&mut stream, &bnum.to_le_bytes(), WRITE_TIMEOUT) {
                                                        let mut sync_theirnum = u64::from_le_bytes(m) + 1;
                                                        // let mut sync_theirnum = u64::from_le_bytes(m);
                                                        println!("{}",format!("syncing them from {}",sync_theirnum).blue());
                                                        loop {

                                                            match NextBlock::read(&sync_theirnum) {
                                                                Ok(x) => {
                                                                    println!("{}: {} bytes",sync_theirnum,x.len());
                                                                    if !stream.write(&(x.len() as u64).to_le_bytes()).is_ok() || !stream.write(&x).is_ok() {println!("Couldn't write block");break}
                                                                    // if !write_timeout(&mut stream, &(x.len() as u64).to_le_bytes(), WRITE_TIMEOUT) {
                                                                    //     break
                                                                    // }
                                                                    // if !write_timeout(&mut stream, &x, WRITE_TIMEOUT) {
                                                                    //     break
                                                                    // }
                                                                },
                                                                Err(x) => ()//println!("Err: {}",x)
                                                            }
                                                            // if let Ok(x) = NextBlock::read(&sync_theirnum) {
                                                            //     if !stream.write(&(x.len() as u64).to_le_bytes()).is_ok() || !stream.write(&x).is_ok() {break}
                                                            //     // if !write_timeout(&mut stream, &(x.len() as u64).to_le_bytes(), WRITE_TIMEOUT) {
                                                            //     //     break
                                                            //     // }
                                                            //     // if !write_timeout(&mut stream, &x, WRITE_TIMEOUT) {
                                                            //     //     break
                                                            //     // }
                                                            // }
                                                            if sync_theirnum == bnum {
                                                                break
                                                            }
                                                            sync_theirnum += 1;
                                                        } 
                                                    }
                                                }
                                                stream.flush();
                                                *cli.write() -= 1;
                                            });
                                        }
                                    } else {
                                        self.spammers.insert(sa);
                                        println!("{}","the user sent spam".bright_yellow().bold());
                                    }
                                }
                            }
                        }
                    }
                    did_something = true;
                }
                // jhgfjhfgj


                // recieved a message as a non comittee member
                while let Async::Ready(Some(fullmsg)) = track_try_unwrap!(self.outer.poll()) {
                    let msg = fullmsg.message.clone();
                    let mut m = msg.payload.to_vec();
                    if let Some(mtype) = m.pop() {
                        println!("{}",format!("got a gossip of type {}!", mtype).green());


                        if mtype == 0 /* transaction someone wants to make */ {
                            // to avoid spam
                            if m.len() < 100_000 {
                                if let Ok(t) = bincode::deserialize::<PolynomialTransaction>(&m) {
                                    let ok = {
                                        if t.inputs.last() == Some(&0u8) {
                                            let bloom = self.bloom.borrow();
                                            t.tags.iter().all(|y| !bloom.contains(y.as_bytes())) && t.verify().is_ok()
                                        } else if t.inputs.last() == Some(&1u8) {
                                            t.verifystk(&self.stkinfo,self.bnum/NONCEYNESS).is_ok()
                                        } else if t.inputs.last() == Some(&2u8) {
                                            t.verifynonanony(&self.nonanony,self.bnum/NONCEYNESS).is_ok()
                                        } else {
                                            false
                                        }
                                    };
                                    if ok {
                                        println!("{}","a staker sent a tx".green());
                                        self.txses.insert(t);
                                        self.outer.handle_gossip_now(fullmsg, true);

                                        continue
                                    }
                                }
                            }
                        } else if mtype == 3 /* recieved a full block */ {
                            if let Ok(lastblock) = bincode::deserialize::<NextBlock>(&m) {
                                let s = self.readblock(lastblock, m, true);
                                self.outer.handle_gossip_now(fullmsg, s);
                                if self.mine.len() >= ACCOUNT_COMBINE && s {
                                    self.gui_sender.send(vec![8]).unwrap();
                                }
                                println!("{}","you have recieved a full block".green());

                                continue
                            }
                        } else if mtype == 97 /* a */ {
                            println!("{}","a staker is trying to connect".green());
                            self.outer.plumtree_node.lazy_push_peers.insert(fullmsg.sender);
                            self.outer.dm(bincode::serialize(&self.stkinfo.vec).unwrap().into_iter().chain(vec![98u8]).collect::<Vec<u8>>(), &[fullmsg.sender], false);

                            continue
                        } else if mtype == 98 /* b */ {
                            let y = self.smine.map(|x|x.0);
                            if self.stkinfo.vec.par_iter().enumerate().all(|(i,(_,(_,x)))| x.is_none() || y == Some(i)) {
                                if let Ok(x) = bincode::deserialize(&m) {
                                    println!("{}","you have connected to the network".green());
                                    let stkinfo = VecHashMap::<CompressedRistretto, (u64, Option<SocketAddr>)>::from(x);
                                    self.stkinfo.vec.par_iter_mut().zip(stkinfo.vec).for_each(|((_,x),(_,y))| {
                                        if x.1.is_none() {
                                            x.1 = y.1;
                                        }
                                    });
                                    continue
                                }
                            }
                        } else if mtype == 110 /* n */ {
                            if let Some(who) = Signature::recieve_signed_message2(&mut m) {
                                if let Ok(m) = bincode::deserialize::<IpAddr>(&m) {
                                    if self.stkinfo.hashmap.contains_key(&who) {
                                        self.stkinfo.mut_value_from_key(&who).1 = Some(SocketAddr::new(m, OUTSIDER_PORT));
                                        println!("{}","a staker has announced their ip".green());
                                    }
                                    self.outer.handle_gossip_now(fullmsg, true);

                                    continue
                                }
                            }
                        }
                        /* spam */
                        println!("{}",format!("a staker ({}) sent spam ({} bytes)",fullmsg.sender,m.len()).bright_yellow().bold());
                        self.outer.kill(&fullmsg.sender);
                        self.outer.handle_gossip_now(fullmsg, false);
                    }
                    did_something = true;
                }
            // }













            /*_________________________________________________________________________________________________
            USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF |||||||||||||
            ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF
            USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF |||||||||||||
            ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF
            USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF |||||||||||||
            ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF
            USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF |||||||||||||
            ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF
            */
            // interacting with the gui
            while let Ok(mut m) = self.gui_reciever.try_recv() {
                println!("got message from gui!\n{}",String::from_utf8_lossy(&m));
                if let Some(istx) = m.pop() {
                    let mut validtx = true;
                    if istx == 33 /* ! */ { // a transaction
                        let txtype = m.pop().unwrap();
                        if txtype == 33 /* ! */ {
                            self.ringsize = m.pop().unwrap();
                        }
                        let mut outs = vec![];
                        while m.len() > 0 {
                            let mut pks = vec![];
                            for _ in 0..3 { // read the pk address
                                if m.len() >= 64 {
                                    let h1 = m.drain(..32).collect::<Vec<_>>().iter().map(|x| (x-97)).collect::<Vec<_>>();
                                    let h2 = m.drain(..32).collect::<Vec<_>>().iter().map(|x| (x-97)*16).collect::<Vec<_>>();
                                    if let Ok(p) = h1.into_iter().zip(h2).map(|(x,y)|x+y).collect::<Vec<u8>>().try_into() {
                                        if let Some(p) = CompressedRistretto(p).decompress() {
                                            pks.push(p);
                                        } else {
                                            pks.push(RISTRETTO_BASEPOINT_POINT);
                                            validtx = false;
                                            println!("an address is invalid");
                                        }
                                    } else {
                                        pks.push(RISTRETTO_BASEPOINT_POINT);
                                        validtx = false;
                                    }
                                } else {
                                    pks.push(RISTRETTO_BASEPOINT_POINT);
                                    validtx = false;
                                }
                            }
                            if m.len() >= 8 && validtx {
                                if let Ok(x) = m.drain(..8).collect::<Vec<_>>().try_into() {
                                    let x = u64::from_le_bytes(x);
                                    // // println!("amounts {:?}",x);
                                    // let y = x/2u64.pow(BETA as u32);
                                    // // println!("need to split this up into {} txses!",y);
                                    // let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                                    // let mut tot = Scalar::zero();
                                    // for _ in 0..y {
                                    //     let amnt = Scalar::from(x/y);
                                    //     tot += amnt;
                                    //     outs.push((recv,amnt));
                                    // }
                                    // let amnt = Scalar::from(x) - tot;
                                    let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                                    outs.push((recv,Scalar::from(x)));
                                } else {
                                    let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                                    let amnt = Scalar::zero();
                                    outs.push((recv,amnt));
                                    validtx = false;
                                }
                            } else {
                                let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                                let amnt = Scalar::zero();
                                outs.push((recv,amnt));
                                validtx = false;
                            }
                        }

                        let mut txbin: Vec<u8>;
                        if txtype == 33 /* ! */ && validtx { // transaction should be spent with unstaked money
                            let loc = self.mine.iter().map(|(&x,_)|x).collect::<Vec<_>>();

                            let m = self.mine.iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>() - outs.iter().map(|x| x.1).sum::<Scalar>();
                            let x = outs.len() - 1;
                            outs[x].1 = m;


                            let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + self.ringsize as u16), &self.height);
                            if let Ok(ring) = recieve_ring(&rname) {
                                // println!("ring: {:?}",ring);
                                // println!("mine: {:?}",acc.iter().map(|x|x.pk.compress()).collect::<Vec<_>>());
                                // println!("ring: {:?}",ring.iter().map(|x|OTAccount::summon_ota(&History::get(&x)).pk.compress()).collect::<Vec<_>>());
                                let rlring = ring.into_iter().map(|x| {
                                    if let Some(x) = self.mine.get(&x) {
                                        x.clone()
                                    } else {
                                        OTAccount::summon_ota(&History::get(&x))
                                    }
                                }).collect::<Vec<OTAccount>>();
                                // println!("ring len: {:?}",rlring.len());
                                let tx = Transaction::spend_ring(&rlring, &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                                let tx = tx.polyform(&rname);
                                if tx.verify().is_ok() {
                                    self.lasttags.push(tx.tags[0]);
                                    txbin = bincode::serialize(&tx).unwrap();
                                    self.txses.insert(tx);
                                } else {
                                    txbin = vec![];
                                    println!("you can't make that transaction!");
                                }
                            } else {
                                txbin = vec![];
                                println!("you aren't allowed to use a ring that big!");
                            }
                            
                        } else if txtype == 63 /* ? */ && validtx { // transaction should be spent with staked money
                            if let Some(smine) = self.smine {
                                let m = Scalar::from(smine.1) - outs.iter().map(|x| x.1).sum::<Scalar>();
                                let x = outs.len() - 1;
                                outs[x].1 = m;
    
                                
                                let (loc, amnt): (Vec<usize>,Vec<u64>) = self.smine.iter().map(|x|x.clone()).unzip();
                                let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                                let tx = Transaction::spend_ring_nonce(&inps, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>(),self.bnum/NONCEYNESS);
                                // tx.verify().unwrap();
                                let mut loc = loc.into_iter().map(|x| (x as u64).to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                                loc.push(1);
                                let tx = tx.polyform(&loc); // push 0
                                if tx.verifystk(&self.stkinfo, self.bnum/NONCEYNESS).is_ok() {
                                    txbin = bincode::serialize(&tx).unwrap();
                                    self.laststk = Some(smine.0);
                                    self.txses.insert(tx);
                                } else {
                                    txbin = vec![];
                                    println!("you can't make that transaction!");
                                }
                            } else {
                                txbin = vec![];
                                println!("you can't make that transaction!");
                            }
                        } else if txtype == 64 /* ?+1 */ && validtx { // transaction should be spent with nonanony money
                            if let Some(nmine) = self.nmine {
                                let m = Scalar::from(nmine.1) - outs.iter().map(|x| x.1).sum::<Scalar>();
                                let x = outs.len() - 1;
                                outs[x].1 = m;
    
                                
                                let (loc, amnt): (Vec<usize>,Vec<u64>) = self.nmine.iter().map(|&x|x).unzip();
                                let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                                let tx = Transaction::spend_ring_nonce(&inps, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>(),self.bnum/NONCEYNESS);
                                tx.verify_nonce(self.bnum/NONCEYNESS).unwrap();
                                let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                                loc.push(2);
                                let tx = tx.polyform(&loc); // push 0
                                println!("{:?}",self.nonanony);
                                if tx.verifynonanony(&self.nonanony,self.bnum/NONCEYNESS).is_ok() {
                                    txbin = bincode::serialize(&tx).unwrap();
                                    self.lastnonanony = Some(nmine.0);
                                    self.txses.insert(tx);
                                } else {
                                    txbin = vec![];
                                    println!("you can't make that transaction!");
                                }
                            } else {
                                txbin = vec![];
                                println!("you have non nonanony money!");
                            }
                        } else {
                            txbin = vec![];
                            println!("somethings wrong with your query!");

                        }
                        // if that tx is valid and ready as far as you know
                        if validtx && !txbin.is_empty() {
                            txbin.push(0);
                            self.outer.broadcast_now(txbin);
                            println!("transaction broadcasted");
                        } else {
                            println!("transaction not made right now");
                        }
                    } else if istx == 2 /* divide accounts so you can make faster tx */ {


                        let fee = Scalar::from(u64::from_le_bytes(m.try_into().unwrap()));


                        for mine in self.mine.clone().iter().collect::<Vec<_>>().chunks(ACCOUNT_COMBINE) {
                            if mine.len() > 1 {
                                let (loc, acc): (Vec<u64>,Vec<OTAccount>) = mine.iter().map(|x|(*x.0,x.1.clone())).unzip();
    
                                let mymoney = acc.iter().map(|x| x.com.amount.unwrap()).sum::<Scalar>();
                                let outs = vec![(self.me,mymoney-fee)];
    






                                let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + self.ringsize as u16), &self.height);
                                let ring = recieve_ring(&rname).unwrap();
                                // println!("ring: {:?}",ring);
                                // println!("mine: {:?}",acc.iter().map(|x|x.pk.compress()).collect::<Vec<_>>());
                                // println!("ring: {:?}",ring.iter().map(|x|OTAccount::summon_ota(&History::get(&x)).pk.compress()).collect::<Vec<_>>());
                                let rlring = ring.iter().map(|x| {
                                    if let Some(x) = self.mine.get(&x) {
                                        x.clone()
                                    } else {
                                        OTAccount::summon_ota(&History::get(&x))
                                    }
                                }).collect::<Vec<OTAccount>>();
                                // println!("ring len: {:?}",rlring.len());
                                let tx = Transaction::spend_ring(&rlring, &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                                let tx = tx.polyform(&rname);
                                



                                println!("ring:----------------------------------\n{:?}",ring);
                                if tx.verify().is_ok() {
                                    let mut txbin = bincode::serialize(&tx).unwrap();
                                    txbin.push(0);
                                    self.lasttags.push(tx.tags[0]);
                                    self.outer.broadcast_now(txbin);
                                    println!("{}","==========================\nTRANDACTION SENT\n==========================".bright_yellow().bold());
                                } else {
                                    println!("{}","TRANSACTION INVALID".red().bold());
                                }
                            }
                        }
                        println!("{}","done with that".green());


                    } else if istx == u8::MAX /* panic button */ {
                        
                        let fee = u64::from_le_bytes(m.drain(..8).collect::<Vec<_>>().try_into().unwrap());
                        let newacc = Account::new(&format!("{}",String::from_utf8_lossy(&m)));

                        let mut amnt = self.mine.iter().map(|x| u64::from_le_bytes(x.1.com.amount.unwrap().as_bytes()[..8].try_into().unwrap())).sum::<u64>();
                        if amnt >= fee {
                            amnt -= fee;
                        }
                        let mut stkamnt = self.smine.iter().map(|x| x.1).sum::<u64>();
                        if stkamnt >= fee {
                            stkamnt -= fee;
                        }
                        let mut nonanony = self.nmine.iter().map(|x| x.1).sum::<u64>();
                        if nonanony >= fee {
                            nonanony -= fee;
                        }

                        self.paniced = Some((self.me.clone(),None,fee));
                        // send unstaked money
                        if self.mine.len() > 0 {
                            let (loc, _acc): (Vec<u64>,Vec<OTAccount>) = self.mine.iter().map(|x|(x.0,x.1.clone())).unzip();

                            // println!("remembered owned accounts");
                            let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16), &self.height);
                            let ring = recieve_ring(&rname).unwrap();

                            // println!("made rings");
                            /* you don't use a ring for panics (the ring is just your own accounts) */ 
                            let mut rlring = ring.iter().map(|&x| self.mine.iter().filter(|(&y,_)| y == x).collect::<Vec<_>>()[0].1.clone()).collect::<Vec<OTAccount>>();
                            let me = self.me;
                            rlring.iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x) {*x = y;});
                            
                            let mut outs = vec![];
                            outs.push((&newacc,Scalar::from(amnt)));
                            let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x| (x.0,&x.1)).collect());

                            println!("{:?}",rlring.iter().map(|x| x.com.amount).collect::<Vec<_>>());
                            println!("{:?}",amnt);
                            let tx = tx.polyform(&rname);
                            if tx.verify().is_ok() {
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                txbin.push(0);
                                self.paniced.iter_mut().for_each(|x| {
                                    x.1 = Some(tx.clone())
                                });
                                self.txses.insert(tx);
                                self.outer.broadcast_now(txbin.clone());
                                // println!("transaction made!");
                            } else {
                                println!("you can't make that transaction, user!");
                            }
                        }

                        // send staked money
                        if let Some(smine) = &self.smine {
                            let (loc, amnt) = smine.clone();
                            let inps = self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(amnt))).unwrap();


                            let mut outs = vec![];
                            outs.push((&newacc,Scalar::from(stkamnt)));

                            
                            let tx = Transaction::spend_ring_nonce(&vec![inps], &outs.iter().map(|x| (x.0,&x.1)).collect(),self.bnum/NONCEYNESS);
                            // println!("about to verify!");
                            // tx.verify().unwrap();
                            // println!("finished to verify!");
                            let mut loc = loc.to_le_bytes().to_vec();
                            loc.push(1);
                            let tx = tx.polyform(&loc); // push 0
                            if tx.verifystk(&self.stkinfo,self.bnum/NONCEYNESS).is_ok() {
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                self.txses.insert(tx);
                                txbin.push(0);
                                self.outer.broadcast_now(txbin.clone());
                                println!("sending tx!");
                            } else {
                                println!("you can't make that transaction!");
                            }
                        }

                        // send staked money
                        if let Some(nmine) = &self.nmine {
                            let (loc, amnt) = nmine;
                            let inps = self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(*amnt))).unwrap();


                            let mut outs = vec![];
                            outs.push((&newacc,Scalar::from(nonanony)));

                            
                            let tx = Transaction::spend_ring_nonce(&vec![inps], &outs.iter().map(|x| (x.0,&x.1)).collect(),self.bnum/NONCEYNESS);
                            // println!("about to verify!");
                            // tx.verify().unwrap();
                            // println!("finished to verify!");
                            let mut loc = loc.to_le_bytes().to_vec();
                            loc.push(2);
                            let tx = tx.polyform(&loc); // push 0
                            if tx.verifynonanony(&self.nonanony,self.bnum/NONCEYNESS).is_ok() {
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                self.txses.insert(tx);
                                txbin.push(0);
                                self.outer.broadcast_now(txbin.clone());
                                println!("sending tx!");
                            } else {
                                println!("you can't make that transaction!");
                            }
                        }


                        self.mine = HashMap::new();
                        self.reversemine = HashMap::new();
                        self.alltagsever = HashSet::new();
                        self.smine = None;
                        self.me = newacc;
                        self.key = self.me.stake_acc().receive_ot(&self.me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap().sk.unwrap();
                        self.keylocation = None;


                        let mut info = bincode::serialize(&
                            vec![
                                bincode::serialize(&self.me.name()).unwrap(),
                                bincode::serialize(&self.me.stake_acc().name()).unwrap(),
                                bincode::serialize(&self.me.nonanony_acc().name()).unwrap(),
                                bincode::serialize(&self.me.sk.as_bytes().to_vec()).unwrap(),
                                bincode::serialize(&self.me.vsk.as_bytes().to_vec()).unwrap(),
                                bincode::serialize(&self.me.ask.as_bytes().to_vec()).unwrap(),
                            ]
                        ).unwrap();
                        info.push(254);
                        self.gui_sender.send(info).unwrap();


                    } else if istx == 121 /* y */ { // you clicked sync
                        self.attempt_sync(None, false);
                    } else if istx == 42 /* * */ { // entry address
                        self.headshard = 0;
                        self.usurpingtime = Instant::now();
                        self.gui_sender.send(vec![blocktime(self.bnum as f64) as u8,128]).unwrap();
                        let mut ip = String::from_utf8_lossy(&m);
                        ip.to_mut().retain(|c| !c.is_whitespace());
                        if let Ok(socket) = format!("{}:{}",ip,OUTSIDER_PORT).parse() {
                            println!("ip: {}",ip);
                            self.attempt_sync(Some(socket), true);
                            self.outer.dm(vec![97],&[NodeId::new(format!("{}:{}",ip,DEFAULT_PORT).parse().unwrap(), LocalNodeId::new(0))],true);
                        } else {
                            println!("that's not an ip address!");
                        }
                    } else if istx == 64 /* @ */ {
                        let mut friend = self.outer.plumtree_node().all_push_peers();
                        friend.remove(self.outer.plumtree_node().id());
                        println!("{:?}",friend);
                        let friend = friend.into_iter().collect::<Vec<_>>();
                        println!("friends: {:?}",friend);
                        let mut gm = (friend.len() as u16).to_le_bytes().to_vec();
                        gm.push(4);
                        self.gui_sender.send(gm).unwrap();
                    } else if istx == 0 {
                        println!("Saving!");
                        self.save();
                        self.gui_sender.send(vec![253]).unwrap();
                    } else if istx == 98 /* b */ {
                        self.maxcli = m[0];
                    }
                }
            }












        }
        Ok(Async::NotReady)
    }
}
