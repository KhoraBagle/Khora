#[macro_use]
// #[allow(unreachable_code)]
extern crate trackable;

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use khora::seal::BETA;
use parking_lot::{RwLock, Mutex};
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
use std::time::{Duration, Instant};
use std::borrow::Borrow;
use khora::transaction::*;
use curve25519_dalek::ristretto::{CompressedRistretto};
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use khora::bloom::*;
use khora::validation::*;
use khora::ringmaker::*;
use serde::{Serialize, Deserialize};
use khora::validation::{
    NUMBER_OF_VALIDATORS, SIGNING_CUTOFF, QUEUE_LENGTH, REPLACERATE, PERSON0,
    STAKER_BLOOM_NAME, STAKER_BLOOM_SIZE, STAKER_BLOOM_HASHES,
    reward, blocktime
};

use gip::{Provider, ProviderDefaultV4};
use local_ip_address::local_ip;
use colored::Colorize;


/// when to announce you're about to be in the comittee or how far in advance you can no longer serve as leader
const EXIT_TIME: usize = REPLACERATE*5;
/// amount of seconds to wait before initiating shard takeover
const USURP_TIME: u64 = 3600;
/// the default port
const DEFAULT_PORT: u16 = 8334;
/// the outsider port
const OUTSIDER_PORT: u16 = 8335;

fn main() -> Result<(), MainError> {
    let logger = track!(TerminalLoggerBuilder::new().destination(Destination::Stderr).level("info".parse().unwrap()).build())?; // info or debug


    let outerlistener = TcpListener::bind(format!("0.0.0.0:{}",OUTSIDER_PORT)).unwrap();

    
    // let local_socket: SocketAddr = format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap();
    let local_socket: SocketAddr = format!("{}:{}",local_ip().unwrap(),DEFAULT_PORT).parse().unwrap();
    let mut p = ProviderDefaultV4::new();
    let global_addr = match p.get_addr() {
        Ok(x) => x.v4addr.unwrap(),
        Err(x) => {print!("Can't get global ip! Exiting because error: "); panic!("{}",x);},
    };
    // let global_addr = local_ip().unwrap(); // <----------this would be for local. uncomment above for global
    let global_socket = format!("{}:{}",global_addr,DEFAULT_PORT).parse::<SocketAddr>().unwrap();
    println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);

    // this is the number of shards they keep track of
    let max_shards = 64usize; /* this if for testing purposes... there IS NO MAX SHARDS */
    



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
                    stream.set_read_timeout(Some(Duration::from_millis(500)));
                    stream.set_write_timeout(Some(Duration::from_millis(500)));
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
        // everyone agrees this person starts with 1 khora token
        let initial_history = (PERSON0,1u64);
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
    
            let mut allnetwork = HashMap::new();
            allnetwork.insert(initial_history.0, (0u64,None));
            let (smine, keylocation) = {
                if initial_history.0 == me.stake_acc().derive_stk_ot(&Scalar::from(initial_history.1)).pk.compress() {
                    println!("\n\nhey i guess i founded this crypto!\n\n");
                    allnetwork.insert(initial_history.0, (0u64,Some(SocketAddr::new(global_socket.ip(),OUTSIDER_PORT))));
                    (Some([0u64,initial_history.1]), Some(0))
                } else {
                    (None, None)
                }
            };
    
            // creates the node with the specified conditions then saves it to be used from now on
            let node = KhoraNode {
                inner: NodeBuilder::new().finish( ServiceBuilder::new(format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap()).finish(ThreadPoolExecutor::new().unwrap().handle(), SerialLocalNodeIdGenerator::new()).handle()),
                outer: NodeBuilder::new().finish( ServiceBuilder::new(format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap()).finish(ThreadPoolExecutor::new().unwrap().handle(), SerialLocalNodeIdGenerator::new()).handle()),
                outerlister: channel::unbounded().1,
                allnetwork,
                lasttags: vec![],
                lastspot: None,
                me,
                mine: HashMap::new(),
                reversemine: HashMap::new(),
                smine: smine.clone(), // [location, amount]
                key,
                keylocation,
                leader: PERSON0,
                overthrown: HashSet::new(),
                votes: vec![0;NUMBER_OF_VALIDATORS],
                stkinfo: vec![initial_history.clone()],
                queue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|_| 0).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                exitqueue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                comittee: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|_| 0).collect::<Vec<usize>>()).collect::<Vec<_>>(),
                lastname: Scalar::one().as_bytes().to_vec(),
                bloom: BloomFile::from_randomness(STAKER_BLOOM_NAME, STAKER_BLOOM_SIZE, STAKER_BLOOM_HASHES, true),
                bnum: 0u64,
                lastbnum: 0u64,
                height: 0u64,
                sheight: 1u64,
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
                newest: 0u64,
                gui_sender: usend.clone(),
                gui_reciever: channel::unbounded().1,
                moneyreset: None,
                oldstk: None,
                cumtime: 0f64,
                blocktime: blocktime(0.0),
                lightning_yielder,
                ringsize: 5,
                gui_timer: Instant::now(),
                clients: Arc::new(RwLock::new(0)),
                maxcli: 10,
                spammers: HashSet::new(),
            };
            node.save();

            let mut info = bincode::serialize(&
                vec![
                bincode::serialize(&node.me.name()).expect("should work"),
                bincode::serialize(&node.me.stake_acc().name()).expect("should work"),
                bincode::serialize(&node.me.sk.as_bytes().to_vec()).expect("should work"),
                bincode::serialize(&node.me.vsk.as_bytes().to_vec()).expect("should work"),
                bincode::serialize(&node.me.ask.as_bytes().to_vec()).expect("should work"),
                ]
            ).expect("should work");
            info.push(254);
            usend.send(info).expect("should work");


            let node = KhoraNode::load(frontnode, backnode, usend, urecv,recvtcp);
            let mut mymoney = node.mine.iter().map(|x| node.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
            mymoney.extend(node.smine.iter().map(|x| x[1]).sum::<u64>().to_le_bytes());
            mymoney.push(0);
            println!("my money: {:?}",node.smine.iter().map(|x| x[1]).sum::<u64>());
            node.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui
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
        mymoney.extend(node.smine.iter().map(|x| x[1]).sum::<u64>().to_le_bytes());
        mymoney.push(0);
        println!("my money: {:?}",node.smine.iter().map(|x| x[1]).sum::<u64>());
        node.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui

        let app = gui::staker::KhoraStakerGUI::new(
            ui_reciever,
            ui_sender,
            node.me.name(),
            node.me.stake_acc().name(),
            node.me.sk.as_bytes().to_vec(),
            node.me.vsk.as_bytes().to_vec(),
            node.me.ask.as_bytes().to_vec(),
            false,
        );
        let native_options = eframe::NativeOptions::default();
        std::thread::spawn(move || {
            node.attempt_sync(None);
            executor.spawn(service.map_err(|e| panic!("{}", e)));
            executor.spawn(node);
    
    
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
    smine: Option<[u64; 2]>, // [location, amount]
    allnetwork: HashMap<CompressedRistretto,(u64,Option<SocketAddr>)>,
    key: Scalar,
    keylocation: Option<u64>,
    leader: CompressedRistretto,
    overthrown: HashSet<CompressedRistretto>,
    votes: Vec<i32>,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bloom: [u128;2],
    bnum: u64,
    lastbnum: u64,
    height: u64,
    sheight: u64,
    alltagsever: HashSet<CompressedRistretto>,
    headshard: usize,
    outer_view: HashSet<NodeId>,
    outer_eager: HashSet<NodeId>,
    av: Vec<NodeId>,
    pv: Vec<NodeId>,
    moneyreset: Option<Vec<u8>>,
    oldstk: Option<(Account, Option<[u64;2]>, u64)>,
    cumtime: f64,
    blocktime: f64,
    lightning_yielder: bool,
    is_validator: bool,
    ringsize: u8,
    lasttags: Vec<CompressedRistretto>,
    lastspot: Option<u64>,
    maxcli: u8,
}

/// the node used to run all the networking
struct KhoraNode {
    inner: Node<Vec<u8>>, // for sending and recieving messages as a validator (as in inner sanctum)
    outer: Node<Vec<u8>>, // for sending and recieving messages as a non validator (as in not inner)
    outerlister: channel::Receiver<TcpStream>, // for listening to people not in the network
    gui_sender: channel::Sender<Vec<u8>>,
    gui_reciever: channel::Receiver<Vec<u8>>,
    allnetwork: HashMap<CompressedRistretto,(u64,Option<SocketAddr>)>,
    me: Account,
    mine: HashMap<u64, OTAccount>,
    reversemine: HashMap<CompressedRistretto, u64>,
    smine: Option<[u64; 2]>, // [location, amount]
    key: Scalar,
    keylocation: Option<u64>,
    leader: CompressedRistretto, // would they ever even reach consensus on this for new people when a dishonest person is eliminated???
    overthrown: HashSet<CompressedRistretto>,
    votes: Vec<i32>,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bloom: BloomFile,
    bnum: u64,
    lastbnum: u64,
    height: u64,
    sheight: u64,
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
    newest: u64,
    moneyreset: Option<Vec<u8>>,
    oldstk: Option<(Account, Option<[u64;2]>, u64)>,
    cumtime: f64,
    blocktime: f64,
    lightning_yielder: bool,
    gui_timer: Instant,
    ringsize: u8,
    lasttags: Vec<CompressedRistretto>,
    lastspot: Option<u64>,
    clients: Arc<RwLock<u8>>,
    maxcli: u8,
    spammers: HashSet<SocketAddr>,
}

impl KhoraNode {
    /// saves the important information like staker state and block number to a file: "myNode"
    fn save(&self) {
        if !self.moneyreset.is_some() && !self.oldstk.is_some() {
            let sn = SavedNode {
                lasttags: self.lasttags.clone(),
                lastspot: self.lastspot.clone(),
                me: self.me,
                mine: self.mine.clone(),
                reversemine: self.reversemine.clone(),
                smine: self.smine.clone(), // [location, amount]
                allnetwork: self.allnetwork.clone(),
                key: self.key,
                keylocation: self.keylocation.clone(),
                leader: self.leader.clone(),
                overthrown: self.overthrown.clone(),
                votes: self.votes.clone(),
                stkinfo: self.stkinfo.clone(),
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
                moneyreset: self.moneyreset.clone(),
                oldstk: self.oldstk.clone(),
                cumtime: self.cumtime,
                blocktime: self.blocktime,
                lightning_yielder: self.lightning_yielder,
                is_validator: self.is_validator,
                ringsize: self.ringsize,
                av: self.outer.hyparview_node.active_view.clone(),
                pv: self.outer.hyparview_node.passive_view.clone(),
                maxcli: self.maxcli,
            }; // just redo initial conditions on the rest
            let mut sn = bincode::serialize(&sn).unwrap();
            let mut f = File::create("myNode").unwrap();
            f.write_all(&mut sn).unwrap();
        }
    }

    /// loads the node information from a file: "myNode"
    fn load(inner: Node<Vec<u8>>, mut outer: Node<Vec<u8>>, gui_sender: channel::Sender<Vec<u8>>, gui_reciever: channel::Receiver<Vec<u8>>, outerlister: channel::Receiver<TcpStream>) -> KhoraNode {
        let mut buf = Vec::<u8>::new();
        let mut f = File::open("myNode").unwrap();
        f.read_to_end(&mut buf).unwrap();

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
            lastspot: sn.lastspot.clone(),
            allnetwork: sn.allnetwork.clone(),
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
            key: sn.key,
            keylocation: sn.keylocation.clone(),
            leader: sn.leader.clone(),
            overthrown: sn.overthrown.clone(),
            votes: sn.votes.clone(),
            stkinfo: sn.stkinfo.clone(),
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
            newest: 0u64,
            moneyreset: sn.moneyreset,
            oldstk: sn.oldstk,
            cumtime: sn.cumtime,
            blocktime: sn.blocktime,
            lightning_yielder: sn.lightning_yielder,
            gui_timer: Instant::now(),
            ringsize: sn.ringsize,
            clients: Arc::new(RwLock::new(0)),
            maxcli: sn.maxcli,
            spammers: HashSet::new(),
        }
    }

    /// reads a full block (by converting it to lightning then reading that)
    fn readblock(&mut self, lastblock: NextBlock, m: Vec<u8>, save: bool) -> bool {
        let lastlightning = lastblock.tolightning();
        let l = bincode::serialize(&lastlightning).unwrap();
        self.readlightning(lastlightning,l,Some(m.clone()), save)
    }

    /// reads a lightning block and saves information when appropriate
    fn readlightning(&mut self, lastlightning: LightningSyncBlock, m: Vec<u8>, largeblock: Option<Vec<u8>>, save: bool) -> bool {
        let t = Instant::now();
        if lastlightning.bnum >= self.bnum {
            let com = self.comittee.par_iter().map(|x| x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>();
            if lastlightning.shards.len() == 0 {
                println!("Error in block verification: there is no shard");
                return false;
            }
            let v = if (lastlightning.shards[0] as usize >= self.headshard) && (lastlightning.last_name == self.lastname) {
                lastlightning.verify_multithread(&com[lastlightning.shards[0] as usize], &self.stkinfo).is_ok()
            } else {
                false
            };
            if v  {
                if save {
                    // saves your current information BEFORE reading the new block. It's possible a leader is trying to cause a fork which can only be determined 1 block later based on what the comittee thinks is real
                    self.save();
                }

                if let Some(k) = &self.keylocation {
                    if lastlightning.info.stkout.contains(k) {
                        self.gui_sender.send(vec![6]).expect("something's wrong with the communication to the gui");
                    }
                }
                if let Some(k) = &mut self.lastspot {
                    if lastlightning.info.stkout.contains(k) {
                        self.gui_sender.send(vec![6]).expect("something's wrong with the communication to the gui");
                    } else {
                        *k -= lastlightning.info.stkout.par_iter().filter(|&&x| {
                            x < *k
                        }).count() as u64;
                    }
                }

                // if you are one of the validators who leave this turn, it is your responcibility to send the block to the outside world
                if let Some(keylocation) = self.keylocation {
                    let c = &self.comittee[self.headshard];
                    if self.exitqueue[self.headshard].par_iter().take(REPLACERATE).map(|&x| c[x]).any(|x| keylocation == x as u64) {
                        if let Some(mut lastblock) = largeblock.clone() {
                            lastblock.push(3);
                            println!("{}",format!("sending block {} to the outside world!",lastlightning.bnum).green());
                            self.outer.broadcast_now(lastblock); /* broadcast the block to the outside world */
                        }
                    }
                }
                self.headshard = lastlightning.shards[0] as usize;

                println!("{}","=========================================================\ngot a block!".magenta());

                self.overthrown.remove(&self.stkinfo[lastlightning.leader.pk as usize].0);
                if self.stkinfo[lastlightning.leader.pk as usize].0 != self.leader {
                    self.overthrown.insert(self.leader);
                }

                // if you're synicng, you just infer the empty blocks that no one saves
                for _ in self.bnum..lastlightning.bnum {
                    // println!("I missed a block!");
                    let reward = reward(self.cumtime,self.blocktime);
                    self.cumtime += self.blocktime;
                    self.blocktime = blocktime(self.cumtime);

                    self.gui_sender.send(vec![!NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut self.smine, reward) as u8,1]).expect("there's a problem communicating to the gui!");
                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);

                    if !self.lightning_yielder {
                        NextBlock::save(&vec![]);
                    }
                    LightningSyncBlock::save(&vec![]);

                    // if you're panicing, the transaction you have saved may need to be updated based on if you gain or loose money
                    if let Some(oldstk) = &mut self.oldstk {
                        NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut oldstk.1, reward);
                    }




                    self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                    for i in 0..self.comittee.len() {
                        select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                    }
                    self.bnum += 1;
                }


                println!("none time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());
                // calculate the reward for this block as a function of the current time and scan either the block or an empty block based on conditions
                let reward = reward(self.cumtime,self.blocktime);
                if !(lastlightning.info.txout.is_empty() && lastlightning.info.stkin.is_empty() && lastlightning.info.stkout.is_empty()) {
                    let (mut guitruster, new) = lastlightning.scanstk(&self.me, &mut self.smine, &mut self.sheight, &self.comittee, reward, &self.stkinfo);
                    guitruster = !(lastlightning.scan(&self.me, &mut self.mine, &mut self.reversemine, &mut self.height, &mut self.alltagsever) || guitruster);
                    self.gui_sender.send(vec![guitruster as u8,1]).expect("there's a problem communicating to the gui!");

                    if new {
                        let message = bincode::serialize(&self.outer.plumtree_node().id().address().ip()).unwrap();
                        let mut fullmsg = Signature::sign_message2(&self.key,&message);
                        fullmsg.push(110);
                        self.outer.broadcast_now(fullmsg);
                    }
                    

                    // println!("saving block...");
                    lastlightning.update_bloom(&mut self.bloom,&false);
                    if !self.lightning_yielder {
                        NextBlock::save(&largeblock.unwrap()); // important! if you select to recieve full blocks you CAN NOT recieve with lightning blocks (because if you do youd miss full blocks)
                    }
                    LightningSyncBlock::save(&m);

                    if let Some(x) = &self.smine {
                        self.keylocation = Some(x[0])
                    }
                    lastlightning.scan_as_noone(&mut self.stkinfo, &mut self.allnetwork, &mut self.queue, &mut self.exitqueue, &mut self.comittee, reward, true);

                    self.lastbnum = self.bnum;
                    let mut hasher = Sha3_512::new();
                    hasher.update(m);
                    self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                } else {
                    self.gui_sender.send(vec![!NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut self.smine, reward) as u8,1]).expect("there's a problem communicating to the gui!");
                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);
                    if !self.lightning_yielder {
                        NextBlock::save(&vec![]);
                    }
                    LightningSyncBlock::save(&vec![]);
                }
                println!("some time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());

                self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                self.newest = self.queue[self.headshard][0] as u64;
                for i in 0..self.comittee.len() {
                    select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                }
                self.bnum += 1;

                
                self.votes = self.votes.par_iter().zip(self.comittee[self.headshard].par_iter()).map(|(z,&x)| z + lastlightning.validators.iter().filter(|y| y.pk == x as u64).count() as i32).collect::<Vec<_>>();
                


                

                println!("midd time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());
                
                let s = &self.stkinfo;
                let o = &self.overthrown;
                let cm = &self.comittee[self.headshard];
                /* LEADER CHOSEN BY VOTES (off blockchain, says which comittee member they should send stuff to) */
                let abouttoleave = self.exitqueue[self.headshard].par_iter().take(EXIT_TIME).map(|z| cm[*z].clone()).collect::<HashSet<_>>();
                self.leader = self.stkinfo[*self.comittee[self.headshard].par_iter().zip(self.votes.par_iter()).max_by_key(|(x,&y)| {
                    if abouttoleave.contains(x) || o.contains(&s[**x].0) {
                        i32::MIN
                    } else {
                        y
                    }
                }).unwrap().0].0;
                /* LEADER CHOSEN BY VOTES (off blockchain, says which comittee member they should send stuff to) */

                
                // send info to the gui
                let mut mymoney = self.mine.par_iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
                mymoney.extend(self.smine.unwrap_or_default()[1].to_le_bytes());
                mymoney.push(0);
                self.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui

                let mut thisbnum = self.bnum.to_le_bytes().to_vec();
                thisbnum.push(2);
                self.gui_sender.send(thisbnum).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui

                // println!("block {} name: {:?}",self.bnum, self.lastname);

                // delete the set of overthrone leaders sometimes to give them another chance
                if self.bnum % 128 == 0 {
                    self.overthrown = HashSet::new();
                }
                // if you save the history, the txses you know about matter; otherwise, they don't (becuase you're not involved in block creation)
                let s = self.stkinfo.borrow();
                let bloom = self.bloom.borrow();
                println!("{}",format!("had {} tx",self.txses.len()).magenta());
                println!("{}",format!("block had {} stkin, {} stkout, {} otain",lastlightning.info.stkin.len(),lastlightning.info.stkin.len(),lastlightning.info.txout.len()).magenta());
                println!("late time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());
                self.txses = self.txses.par_iter().filter(|x| {
                    if x.inputs.last() == Some(&1) {
                        x.verifystk(s).is_ok()
                    } else {
                        x.tags.par_iter().all(|x| !bloom.contains(x.as_bytes()))
                    }
                }).cloned().collect();
                println!("most time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());

                println!("{}",format!("have {} tx",self.txses.len()).magenta());
                // runs any operations needed for the panic button to function
                self.send_panic_or_stop(&lastlightning, reward, save);

                // if you're lonely and on the comittee, you try to reconnect with the comittee (WARNING: DOES NOT HANDLE IF YOU HAVE FRIENDS BUT THEY ARE IGNORING YOU)
                if self.is_validator && self.inner.plumtree_node().all_push_peers().is_empty() {
                    for n in self.comittee[self.headshard].iter().filter_map(|&x| self.allnetwork.get(&self.stkinfo[x].0).unwrap().1).collect::<Vec<_>>() {
                        self.inner.join(NodeId::new(SocketAddr::from((n.ip(),DEFAULT_PORT)),LocalNodeId::new(1)));
                    }
                }


                if self.lasttags.par_iter().any(|x| {
                    lastlightning.info.tags.contains(&x)
                }) {
                    self.lasttags = vec![];
                    self.gui_sender.send(vec![5]);
                }



                self.cumtime += self.blocktime;
                self.blocktime = blocktime(self.cumtime);

                self.gui_sender.send(vec![self.blocktime as u8,128]).expect("something's wrong with the communication to the gui");

                self.sigs = vec![];
                self.doneerly = self.timekeeper;
                self.waitingforentrybool = true;
                self.waitingforleaderbool = false;
                self.waitingforleadertime = Instant::now();
                self.waitingforentrytime = Instant::now();
                self.timekeeper = Instant::now();
                self.usurpingtime = Instant::now();
                // println!("block reading process done!!!");










                // this section tells you if you're on the comittee or not
                // if you're on the comittee you need to pull on your inner node
                // if you're not you need to poll on your user node
                self.is_node = true;
                self.is_validator = false;
                if let Some(keylocation) = self.keylocation {
                    let keylocation = keylocation as usize;
                    self.is_validator = self.comittee[self.headshard].par_iter().any(|x| *x == keylocation);
                    self.gui_sender.send(vec![self.is_validator as u8,3]).unwrap();


                    if self.queue[self.headshard].par_iter().take(REPLACERATE).any(|&x| x == keylocation) {
                        // announce yourself to the comittee because it's about to be your turn
                        let nw = &self.allnetwork;
                        let si = &self.stkinfo;
                        for n in self.comittee[self.headshard].par_iter().filter_map(|&x| nw.get(&si[x].0).unwrap().1).collect::<Vec<_>>() {
                            self.inner.join(NodeId::new(SocketAddr::from((n.ip(),DEFAULT_PORT)),LocalNodeId::new(1)));
                        }
                    } else {
                        self.is_node = false;
                    }
                }


                println!("full time: {}",format!("{}",t.elapsed().as_millis()).bright_yellow());
                println!("known validator's ips: {:?}", self.allnetwork.iter().filter_map(|(_,(_,x))| x.clone()).collect::<Vec<_>>());
                return true
            }
        }
        false
    }

    /// runs the operations needed for the panic button to work
    fn send_panic_or_stop(&mut self, lastlightning: &LightningSyncBlock, reward: f64, send: bool) {
        if self.moneyreset.is_some() || self.oldstk.is_some() {
            if self.mine.len() < (self.moneyreset.is_some() as usize + self.oldstk.is_some() as usize) {
                let mut oldstkcheck = false;
                if let Some(oldstk) = &mut self.oldstk {
                    if !self.mine.iter().all(|x| x.1.com.amount.unwrap() != Scalar::from(oldstk.2)) {
                        oldstkcheck = true;
                    }
                    if !(lastlightning.info.stkout.is_empty() && lastlightning.info.stkin.is_empty() && lastlightning.info.txout.is_empty()) {
                        lastlightning.scanstk(&oldstk.0, &mut oldstk.1, &mut self.sheight.clone(), &self.comittee, reward, &self.stkinfo);
                    } else {
                        NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut oldstk.1, reward);
                    }
                    oldstk.2 = oldstk.1.iter().map(|x| x[1]).sum::<u64>(); // maybe add a fee here?
                    let (loc, amnt): (Vec<u64>,Vec<u64>) = oldstk.1.iter().map(|x|(x[0],x[1])).unzip();
                    let inps = amnt.into_iter().map(|x| oldstk.0.receive_ot(&oldstk.0.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                    let mut outs = vec![];
                    let y = oldstk.2/2u64.pow(BETA as u32);
                    let mut tot = Scalar::zero();
                    for _ in 0..y {
                        let amnt = Scalar::from(oldstk.2/y);
                        tot += amnt;
                        outs.push((self.me,amnt));
                    }
                    let amnt = Scalar::from(oldstk.2) - tot;
                    outs.push((self.me,amnt));
                    let tx = Transaction::spend_ring(&inps, &outs.iter().map(|x|(&x.0,&x.1)).collect());
                    println!("about to verify!");
                    tx.verify().unwrap();
                    println!("finished to verify!");
                    let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                    loc.push(1);
                    let tx = tx.polyform(&loc); // push 0
                    if tx.verifystk(&self.stkinfo).is_ok() {
                        let mut txbin = bincode::serialize(&tx).unwrap();
                        self.txses.insert(tx);
                        txbin.push(0);
                        if send {
                            self.outer.broadcast_now(txbin);
                        }
                    }
                }
                if oldstkcheck {
                    self.oldstk = None;
                }
                if self.mine.len() > 0 && self.oldstk.is_some() {
                    self.moneyreset = None;
                }
                if let Some(x) = self.moneyreset.clone() {
                    if send {
                        self.outer.broadcast(x);
                    }
                }
            } else {
                self.moneyreset = None;
            }
        }
    }



    /// returns the responces of each person you sent it to and deletes those who are dead from the view
    fn attempt_sync(&mut self, node: Option<SocketAddr> ) {

        // println!("hi");
        let mut rng = &mut rand::thread_rng();
        let mut sendview = if let Some(x) = node {
            vec![x]
        } else {
            let mut x = self.allnetwork.clone();
            x.remove(&self.me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress());
            x.iter().filter_map(|(_,(_,x))| *x).collect::<Vec<_>>()
        };
        
        sendview.shuffle(&mut rng);
        for node in sendview {
            if let Ok(mut stream) =  TcpStream::connect(node) {
                stream.set_read_timeout(Some(Duration::from_millis(500)));
                stream.set_write_timeout(Some(Duration::from_millis(500)));
                if stream.write_all(&[122 - (self.lightning_yielder as u8)]).is_ok() {
                    let mut ok = [0;8];
                    if stream.read_exact(&mut ok).is_ok(){
                        let syncnum = u64::from_le_bytes(ok);
                        self.gui_sender.send(ok.iter().chain(&[7u8]).cloned().collect()).unwrap();
                        let mut blocksize = [0u8;8];
                        let mut counter = 0;
                        while stream.read_exact(&mut blocksize).is_ok() {
                            counter += 1;
                            let bsize = u64::from_le_bytes(blocksize) as usize;
                            let mut serialized_block = vec![0u8;bsize];
                            if stream.read_exact(&mut serialized_block).is_err() {
                                println!("couldn't read the bytes");
                            };
                            if self.lightning_yielder {
                                if let Ok(lastblock) = bincode::deserialize::<LightningSyncBlock>(&serialized_block) {
                                    if !self.readlightning(lastblock, serialized_block, None, (counter%1000 == 0) || (self.bnum >= syncnum-1)) {
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
                    }
                }
            } else {
                println!("your friend is probably busy or you have none");
            }
        }
        // println!("hi");
        self.gui_sender.send([0u8;8].iter().chain(&[7u8]).cloned().collect()).unwrap();
        // println!("hi");

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
                    self.usurpingtime = Instant::now();
                    println!("{}","ready to make block".red());
    
                    // if you are the newest member of the comittee you're responcible for choosing the tx that goes into the next block
                    if self.keylocation == Some(self.newest as u64) {
                        let m = bincode::serialize(&self.txses.par_iter().collect::<Vec<_>>()).unwrap();
                        println!("{}",format!("sending {} tx as newest",self.txses.len()).red().bold());

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
                self.gui_sender.send(gm).expect("should be working");
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
                                if (who == self.newest) || (self.stkinfo[who as usize].0 == self.leader) {
                                    if let Ok(m) = bincode::deserialize::<Vec<PolynomialTransaction>>(&m) {

                                        if let Some(keylocation) = &self.keylocation {
                                            let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, m, &(self.headshard as u16), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo);
                                            let mut m = bincode::serialize(&m).unwrap();
                                            m.push(2);
                                            for _ in self.comittee[self.headshard].iter().filter(|&x|*x as u64 == *keylocation).collect::<Vec<_>>() {
                                                println!("{}","broadcasting block signature".red());
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
                                println!("{}","recieved block signature".red());
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
                    self.leader = self.stkinfo[*self.comittee[0].iter().zip(self.votes.iter()).max_by_key(|(&x,&y)| { // i think it does make sense to not care about whose going to leave soon here
                        let candidate = self.stkinfo[x];
                        if self.overthrown.contains(&candidate.0) {
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
                        if let Ok(lastblock) = NextBlock::finish(&self.key, &self.keylocation.iter().next().unwrap(), &self.sigs, &self.comittee[self.headshard].par_iter().map(|x|*x as u64).collect::<Vec<u64>>(), &(self.headshard as u16), &self.bnum, &self.lastname, &self.stkinfo) {
                            
                            lastblock.verify(&self.comittee[self.headshard].iter().map(|&x| x as u64).collect::<Vec<_>>(), &self.stkinfo).unwrap();
    
                            let mut m = bincode::serialize(&lastblock).unwrap();
                            m.push(3u8);
                            self.inner.broadcast(m);
        
        
                            self.sigs = vec![];
        
                            
                            println!("{}",format!("made a block with {} transactions!",lastblock.txs.len()).red());

                            did_something = true;

                        } else {
                            let comittee = &self.comittee[self.headshard];
                            self.sigs.retain(|x| !comittee.into_par_iter().all(|y| x.leader.pk != *y as u64));
                            let a = self.leader.to_bytes().to_vec();
                            let b = bincode::serialize(&vec![self.headshard as u16]).unwrap();
                            let c = self.bnum.to_le_bytes().to_vec();
                            let d = self.lastname.clone();
                            let e = &self.stkinfo;
                            self.sigs.retain(|x| {
                                let m = vec![a.clone(),b.clone(),Syncedtx::to_sign(&x.txs),c.clone(),d.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
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
                        let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, vec![], &(self.headshard as u16), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo);
                        println!("{}","ATTEMPTING TO MAKE AN EMPTY BLOCK".red().bold());
                        let mut m = bincode::serialize(&m).unwrap();
                        m.push(2);
                        if self.comittee[self.headshard].contains(&(*keylocation as usize)) {
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
                                                let bloom = self.bloom.borrow();
                                                t.tags.iter().all(|y| !bloom.contains(y.as_bytes())) && t.verify().is_ok()
                                            };
                                            if ok {
                                                println!("{}","a user sent a tx".bright_yellow());
                                                m.push(0);
                                                self.outer.broadcast_now(m);
                                                stream.write_all(&[1u8]);
                                            }
                                        }
                                    } else if mtype == 101 /* e */ {
                                        println!("{}","someone wants a bunch of ips".blue());
                                        let x = self.allnetwork.iter().filter_map(|(_,(_,x))| x.clone()).collect::<Vec<_>>();
                                        stream.write_all(&bincode::serialize(&x).unwrap());
                                    } else if mtype == 114 /* r */ { // answer their ring question
                                        if let Ok(r) = recieve_ring(&m) {
                                            println!("{}","someone wants a ring".blue());
                                            thread::spawn(move || {
                                                for i in r {
                                                    if stream.write_all(&History::get_raw(&i)).is_err() {
                                                        break
                                                    }
                                                }
                                            });
                                        }
                                    } else if mtype == 121 /* y */ { // someone sent a sync request
                                        println!("{}","someone wants lightning".blue());
                                        if *self.clients.read() < self.maxcli {
                                            let bnum = self.bnum;
                                            *self.clients.write() += 1;
                                            let cli = self.clients.clone();
                                            thread::spawn(move || {
                                                if let Ok(m) = m.try_into() {
                                                    if stream.write_all(&bnum.to_le_bytes()).is_ok() {
                                                        let mut sync_theirnum = u64::from_le_bytes(m);
                                                        println!("{}",format!("syncing them from {}",sync_theirnum).blue());
                                                        loop {
                                                            if let Ok(x) = LightningSyncBlock::read(&sync_theirnum) {
                                                                println!("{}",x.len());
                                                                if stream.write_all(&(x.len() as u64).to_le_bytes()).is_err() {
                                                                    break
                                                                }
                                                                if stream.write_all(&x).is_err() {
                                                                    break
                                                                }
                                                            }
                                                            sync_theirnum += 1;
                                                            if sync_theirnum == bnum {
                                                                break
                                                            }
                                                        }
                                                    }
                                                }
                                                *cli.write() -= 1;
                                            });
                                        }
                                    } else if mtype == 122 /* z */ { // someone sent a full sync request
                                        println!("{}","someone wants full blocks".blue());
                                        if !self.lightning_yielder && *self.clients.read() < self.maxcli {
                                            let bnum = self.bnum;
                                            *self.clients.write() += 1;
                                            let cli = self.clients.clone();
                                            thread::spawn(move || {
                                                if let Ok(m) = m.try_into() {
                                                    if stream.write_all(&bnum.to_le_bytes()).is_ok() {
                                                        let mut sync_theirnum = u64::from_le_bytes(m);
                                                        println!("{}",format!("syncing them from {}",sync_theirnum).blue());
                                                        loop {
                                                            if let Ok(x) = NextBlock::read(&sync_theirnum) {
                                                                if stream.write_all(&(x.len() as u64).to_le_bytes()).is_err() {
                                                                    break
                                                                }
                                                                if stream.write_all(&x).is_err() {
                                                                    break
                                                                }
                                                            }
                                                            sync_theirnum += 1;
                                                            if sync_theirnum == bnum {
                                                                break
                                                            }
                                                        } 
                                                    }
                                                }
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
                                        if t.inputs.last() == Some(&1) {
                                            t.verifystk(&self.stkinfo).is_ok()
                                        } else {
                                            let bloom = self.bloom.borrow();
                                            t.tags.iter().all(|y| !bloom.contains(y.as_bytes())) && t.verify().is_ok()
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
                                    self.gui_sender.send(vec![8]);
                                }
                                println!("{}","you have recieved a full block".green());

                                continue
                            }
                        } else if mtype == 97 /* a */ {
                            println!("{}","a staker is trying to connect".green());
                            self.outer.plumtree_node.lazy_push_peers.insert(fullmsg.sender);
                            self.outer.dm(bincode::serialize(&self.allnetwork).unwrap().into_iter().chain(vec![98u8]).collect::<Vec<u8>>(), &[fullmsg.sender], false);

                            continue
                        } else if mtype == 98 /* b */ {
                            if self.allnetwork.par_iter().all(|(_,(_,x))| x.is_none()) {
                                if let Ok(x) = bincode::deserialize(&m) {
                                    println!("{}","you have connected to the network".green());
                                    self.allnetwork = x;

                                    continue
                                }
                            }
                        } else if mtype == 110 /* n */ {
                            if let Some(who) = Signature::recieve_signed_message2(&mut m) {
                                if let Ok(m) = bincode::deserialize::<IpAddr>(&m) {
                                    if let Some(x) = self.allnetwork.get_mut(&who) {
                                        x.1 = Some(SocketAddr::new(m, OUTSIDER_PORT));
                                        println!("{}","a staker has announced their ip".green());
                                    }
                                    self.outer.handle_gossip_now(fullmsg, true);

                                    continue
                                }
                            }
                        }
                        /* spam */
                        println!("{}","the staker sent spam".bright_yellow().bold());
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
                            if m.len() >= 8 {
                                if let Ok(x) = m.drain(..8).collect::<Vec<_>>().try_into() {
                                    let x = u64::from_le_bytes(x);
                                    // println!("amounts {:?}",x);
                                    let y = x/2u64.pow(BETA as u32);
                                    // println!("need to split this up into {} txses!",y);
                                    let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                                    let mut tot = Scalar::zero();
                                    for _ in 0..y {
                                        let amnt = Scalar::from(x/y);
                                        tot += amnt;
                                        outs.push((recv,amnt));
                                    }
                                    let amnt = Scalar::from(x) - tot;
                                    outs.push((recv,amnt));
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
                        if txtype == 33 /* ! */ { // transaction should be spent with unstaked money
                            let loc = self.mine.iter().map(|(&x,_)|x).collect::<Vec<_>>();

                            let m = self.mine.iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>() - outs.iter().map(|x| x.1).sum::<Scalar>();
                            let x = outs.len() - 1;
                            outs[x].1 = m;


                            let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + self.ringsize as u16), &self.height);
                            let ring = recieve_ring(&rname).expect("shouldn't fail");
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
                            
                        } else if txtype == 63 /* ? */ { // transaction should be spent with staked money
                            let m = Scalar::from(self.smine.unwrap()[1]) - outs.iter().map(|x| x.1).sum::<Scalar>();
                            let x = outs.len() - 1;
                            outs[x].1 = m;

                            
                            let (loc, amnt): (Vec<u64>,Vec<u64>) = self.smine.iter().map(|x|(x[0] as u64,x[1].clone())).unzip();
                            let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                            let tx = Transaction::spend_ring(&inps, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                            // tx.verify().unwrap();
                            let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                            loc.push(1);
                            let tx = tx.polyform(&loc); // push 0
                            if tx.verifystk(&self.stkinfo).is_ok() {
                                txbin = bincode::serialize(&tx).unwrap();
                                self.txses.insert(tx);
                            } else {
                                txbin = vec![];
                                println!("you can't make that transaction!");
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
                                let ring = recieve_ring(&rname).expect("shouldn't fail");
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
                        let mut stkamnt = self.smine.iter().map(|x| x[1]).sum::<u64>();
                        if stkamnt >= fee {
                            stkamnt -= fee;
                        }
                        // send unstaked money
                        if self.mine.len() > 0 {
                            let (loc, _acc): (Vec<u64>,Vec<OTAccount>) = self.mine.iter().map(|x|(x.0,x.1.clone())).unzip();

                            // println!("remembered owned accounts");
                            let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16), &self.height);
                            let ring = recieve_ring(&rname).expect("shouldn't fail");

                            // println!("made rings");
                            /* you don't use a ring for panics (the ring is just your own accounts) */ 
                            let mut rlring = ring.iter().map(|&x| self.mine.iter().filter(|(&y,_)| y == x).collect::<Vec<_>>()[0].1.clone()).collect::<Vec<OTAccount>>();
                            let me = self.me;
                            rlring.iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x) {*x = y;});
                            
                            let mut outs = vec![];
                            let y = amnt/2u64.pow(BETA as u32);
                            let mut tot = Scalar::zero();
                            for _ in 0..y {
                                let amnt = Scalar::from(amnt/y);
                                tot += amnt;
                                outs.push((&newacc,amnt));
                            }
                            let amnt = Scalar::from(stkamnt) - tot;
                            outs.push((&newacc,amnt));
                            let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x| (x.0,&x.1)).collect());

                            println!("{:?}",rlring.iter().map(|x| x.com.amount).collect::<Vec<_>>());
                            println!("{:?}",amnt);
                            let tx = tx.polyform(&rname);
                            if tx.verify().is_ok() {
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                txbin.push(0);
                                self.lasttags.push(tx.tags[0]);
                                self.txses.insert(tx);
                                self.outer.broadcast_now(txbin.clone());
                                self.moneyreset = Some(txbin);
                                // println!("transaction made!");
                            } else {
                                println!("you can't make that transaction, user!");
                            }
                        }

                        // send staked money
                        if let Some(smine) = &self.smine {
                            let (loc, amnt) = (smine[0],smine[1]);
                            let inps = self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(amnt))).unwrap();


                            let mut outs = vec![];
                            let y = stkamnt/2u64.pow(BETA as u32);
                            let mut tot = 0u64;
                            for _ in 0..y {
                                let stkamnt = Scalar::from(stkamnt/y);
                                tot += amnt;
                                outs.push((&newacc,stkamnt));
                            }
                            let amnt = Scalar::from(stkamnt) - Scalar::from(tot);
                            outs.push((&newacc,amnt));

                            
                            let tx = Transaction::spend_ring(&vec![inps], &outs.iter().map(|x| (x.0,&x.1)).collect());
                            println!("about to verify!");
                            tx.verify().unwrap();
                            println!("finished to verify!");
                            let mut loc = loc.to_le_bytes().to_vec();
                            loc.push(1);
                            let tx = tx.polyform(&loc); // push 0
                            if tx.verifystk(&self.stkinfo).is_ok() {
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                self.txses.insert(tx);
                                txbin.push(0);
                                self.outer.broadcast_now(txbin.clone());
                                self.oldstk = Some((self.me.clone(),self.smine.clone(),stkamnt));
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
                                bincode::serialize(&self.me.name()).expect("should work"),
                                bincode::serialize(&self.me.stake_acc().name()).expect("should work"),
                                bincode::serialize(&self.me.sk.as_bytes().to_vec()).expect("should work"),
                                bincode::serialize(&self.me.vsk.as_bytes().to_vec()).expect("should work"),
                                bincode::serialize(&self.me.ask.as_bytes().to_vec()).expect("should work"),
                            ]
                        ).expect("should work");
                        info.push(254);
                        self.gui_sender.send(info).expect("should work");


                    } else if istx == 121 /* y */ { // you clicked sync
                        self.attempt_sync(None);
                    } else if istx == 42 /* * */ { // entry address
                        let m = format!("{}:{}",String::from_utf8_lossy(&m),DEFAULT_PORT);
                        println!("{}",m);
                        if let Ok(socket) = m.parse() {
                            self.attempt_sync(Some(socket));
                            self.outer.dm(vec![97],&[NodeId::new(socket, LocalNodeId::new(0))],true);
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
                        self.gui_sender.send(gm).expect("should be working");
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
