#[macro_use]
extern crate trackable;

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_COMPRESSED;
use fibers::sync::mpsc;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use kora::seal::BETA;
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use rand::prelude::SliceRandom;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::fs::File;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::Path;
use trackable::error::MainError;
use crossbeam::channel;


use kora::{account::*, gui};
use curve25519_dalek::scalar::Scalar;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::time::{Duration, Instant};
use std::borrow::Borrow;
use kora::transaction::*;
use curve25519_dalek::ristretto::{CompressedRistretto};
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use kora::bloom::*;
use kora::validation::*;
use kora::ringmaker::*;
use serde::{Serialize, Deserialize};
use kora::validation::{NUMBER_OF_VALIDATORS, SIGNING_CUTOFF, QUEUE_LENGTH, REPLACERATE};

use gip::{Provider, ProviderDefaultV4};


/// when to announce you're about to be in the comittee or how far in advance you can no longer serve as leader
const EXIT_TIME: usize = REPLACERATE*5;
/// amount of seconds to wait before initiating shard takeover
const USURP_TIME: u64 = 3600;
/// the default port
const DEFAULT_PORT: u64 = 8334;
/// total money ever produced
const TOTAL_KHORA: f64 = 1.0e16;
/// calculates the amount of time the current block takes to be created
fn blocktime(cumtime: f64) -> f64 {
    // 60f64/(6.337618E-8f64*cumtime+2f64).ln()
    10.0
}
/// calculates the reward for the current block
fn reward(cumtime: f64, blocktime: f64) -> f64 {
    (1.0/(1.653439E-6*cumtime + 1.0) - 1.0/(1.653439E-6*(cumtime + blocktime) + 1.0))*TOTAL_KHORA
}

fn main() -> Result<(), MainError> {
    let logger = track!(TerminalLoggerBuilder::new().destination(Destination::Stderr).level("info".parse().unwrap()).build())?; // info or debug

        



    let local_socket: SocketAddr = format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap();
    let mut p = ProviderDefaultV4::new();
    let global_addr = p.get_addr().unwrap().v4addr.unwrap();
    let global_socket = format!("{}:{}",global_addr,DEFAULT_PORT).parse::<SocketAddr>().unwrap();
    println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);

    // this is the number of shards they keep track of
    let max_shards = 64usize; /* this if for testing purposes... there IS NO MAX SHARDS */
    


    // the myNode file only exists if you already have an account made
    let setup = !Path::new("myNode").exists();
    if setup {
        // everyone agrees this person starts with 1 khora token
        let person0 = CompressedRistretto([46, 235, 227, 188, 55, 53, 9, 126, 167, 207, 202, 101, 150, 150, 172, 207, 209, 208, 211, 52, 47, 206, 19, 115, 199, 189, 202, 10, 56, 220, 138, 55]);
        let initial_history = vec![(person0,1u64)];

        // these are used to communicate with the setup screen
        let (ui_sender_setup, mut urecv_setup) = mpsc::channel();
        let (usend_setup, ui_reciever_setup) = channel::unbounded();

        // creates the setup screen (sets the values used in the loops and sets some gui options)
        let app = gui::KhoraGUI::new(
            ui_reciever_setup,
            ui_sender_setup,
            "".to_string(),
            "".to_string(),
            vec![],
            vec![],
            vec![],
            true,
        );
        let mut native_options = eframe::NativeOptions::default();
        native_options.always_on_top = true;
        eframe::run_native(Box::new(app), native_options);
        println!("You closed the app...");
        let pswrd: Vec<u8>;
        let will_stk: bool;
        let lightning_yielder: bool;
        let wait_to_work = Instant::now();
        loop {
            if wait_to_work.elapsed().as_secs() > 2 {
                panic!("you didn't hit the button you should have");
            }
            if let Async::Ready(Some(m)) = urecv_setup.poll().expect("Shouldn't fail") {
                pswrd = m;
                break
            }
        }
        loop {
            if wait_to_work.elapsed().as_secs() > 2 {
                panic!("you didn't hit the button you should have");
            }
            if let Async::Ready(Some(m)) = urecv_setup.poll().expect("Shouldn't fail") {
                will_stk = m[0] == 0;
                break
            }
        }
        loop {
            if wait_to_work.elapsed().as_secs() > 2 {
                panic!("you didn't hit the button you should have");
            }
            if let Async::Ready(Some(m)) = urecv_setup.poll().expect("Shouldn't fail") {
                lightning_yielder = m[0] == 1;
                break
            }
        }
        println!("{:?}",pswrd);
        let me = Account::new(&pswrd);
        let validator = me.stake_acc().receive_ot(&me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap(); //make a new account
        let key = validator.sk.unwrap();
        let mut keylocation = HashSet::new();
        if will_stk {
            if !lightning_yielder {
                NextBlock::initialize_saving();
            }
            LightningSyncBlock::initialize_saving();
            History::initialize();
            BloomFile::initialize_bloom_file();    
        }
        let bloom = BloomFile::from_randomness();

        let mut smine = vec![];
        for i in 0..initial_history.len() {
            if initial_history[i].0 == me.stake_acc().derive_stk_ot(&Scalar::from(initial_history[i].1)).pk.compress() {
                smine.push([i as u64,initial_history[i].1]);
                keylocation.insert(i as u64);
                println!("\n\nhey i guess i founded this crypto!\n\n");
            }
        }

        // creates the node with the specified conditions then saves it to be used from now on
        let node = KhoraNode {
            inner: NodeBuilder::new().finish( ServiceBuilder::new(format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap()).finish(ThreadPoolExecutor::new().unwrap().handle(), SerialLocalNodeIdGenerator::new()).handle()),
            outer: NodeBuilder::new().finish( ServiceBuilder::new(format!("0.0.0.0:{}",DEFAULT_PORT).parse().unwrap()).finish(ThreadPoolExecutor::new().unwrap().handle(), SerialLocalNodeIdGenerator::new()).handle()),
            save_history: will_stk,
            me,
            mine: HashMap::new(),
            smine: smine.clone(), // [location, amount]
            key,
            keylocation,
            leader: person0,
            overthrown: HashSet::new(),
            votes: vec![0;NUMBER_OF_VALIDATORS],
            stkinfo: initial_history.clone(),
            queue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
            exitqueue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
            comittee: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<Vec<usize>>()).collect::<Vec<_>>(),
            lastname: Scalar::one().as_bytes().to_vec(),
            bloom,
            bnum: 0u64,
            lastbnum: 0u64,
            height: 0u64,
            sheight: initial_history.len() as u64,
            alltagsever: vec![],
            txses: vec![],
            sigs: vec![],
            timekeeper: Instant::now() + Duration::from_secs(1),
            waitingforentrybool: true,
            waitingforleaderbool: false,
            waitingforleadertime: Instant::now(),
            waitingforentrytime: Instant::now(),
            doneerly: Instant::now(),
            headshard: 0,
            usurpingtime: Instant::now(),
            is_validator: !smine.is_empty(),
            is_user: true,
            sent_onces: HashSet::new(),
            knownvalidators: HashMap::new(),
            newest: 0u64,
            rmems: HashMap::new(),
            rname: vec![],
            gui_sender: usend_setup,
            gui_reciever: urecv_setup,
            moneyreset: None,
            sync_returnaddr: None,
            sync_theirnum: 0u64,
            sync_lightning: false,
            outs: None,
            oldstk: None,
            cumtime: 0f64,
            blocktime: blocktime(0.0),
            lightning_yielder,
            gui_timer: Instant::now(),
        };
        node.save();
    }


    println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);

    let executor = track_any_err!(ThreadPoolExecutor::new())?;
    let service = ServiceBuilder::new(local_socket)
        .logger(logger.clone())
        .server_addr(global_socket)
        .finish(executor.handle(), SerialLocalNodeIdGenerator::new());

    let backnode = NodeBuilder::new().logger(logger.clone()).finish(service.handle());
    println!("{:?}",backnode.id());
    let frontnode = NodeBuilder::new().logger(logger).finish(service.handle()); // todo: make this local_id random so people can't guess you
    println!("{:?}",frontnode.id()); // this should be the validator survice



    let (ui_sender, urecv) = mpsc::channel();
    let (usend, ui_reciever) = channel::unbounded();




    let node = KhoraNode::load(frontnode, backnode, usend, urecv);
    let mut mymoney = node.mine.iter().map(|x| node.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
    mymoney.extend(node.smine.iter().map(|x| x[1]).sum::<u64>().to_le_bytes());
    mymoney.push(0);
    println!("my money:\n---------------------------------\n{:?}",mymoney);
    node.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui
    node.save();
    
    


    println!("starting!");
    let app = gui::KhoraGUI::new(
        ui_reciever,
        ui_sender,
        node.me.name(),
        node.me.stake_acc().name(),
        node.me.sk.as_bytes().to_vec(),
        node.me.vsk.as_bytes().to_vec(),
        node.me.ask.as_bytes().to_vec(),
        false,
    );
    println!("starting!");
    let native_options = eframe::NativeOptions::default();
    println!("starting!");
    std::thread::spawn(move || {
        executor.spawn(service.map_err(|e| panic!("{}", e)));
        executor.spawn(node);


        track_any_err!(executor.run()).unwrap();
    });
    eframe::run_native(Box::new(app), native_options);
    println!("ending!");
    // add save node command somehow or add that as exit thing and add wait for saved signal here
    Ok(())
}


#[derive(Clone, Serialize, Deserialize, Debug)]
/// the information that you save to a file when the app is off (not including gui information like saved friends)
struct SavedNode {
    save_history: bool, //just testing. in real code this is true; but i need to pretend to be different people on the same computer
    me: Account,
    mine: HashMap<u64, OTAccount>,
    smine: Vec<[u64; 2]>, // [location, amount]
    key: Scalar,
    keylocation: HashSet<u64>,
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
    alltagsever: Vec<CompressedRistretto>,
    headshard: usize,
    outer_view: Vec<NodeId>,
    rmems: HashMap<u64,OTAccount>,
    rname: Vec<u8>,
    moneyreset: Option<Vec<u8>>,
    oldstk: Option<(Account, Vec<[u64;2]>, u64)>,
    cumtime: f64,
    blocktime: f64,
    lightning_yielder: bool,
    is_validator: bool,
}

/// the node used to run all the networking
struct KhoraNode {
    inner: Node<Vec<u8>>, // for sending and recieving messages as a validator (as in inner sanctum)
    outer: Node<Vec<u8>>, // for sending and recieving messages as a non validator (as in not inner)
    gui_sender: channel::Sender<Vec<u8>>,
    gui_reciever: mpsc::Receiver<Vec<u8>>,
    save_history: bool, //just testing. in real code this is true; but i need to pretend to be different people on the same computer
    me: Account,
    mine: HashMap<u64, OTAccount>,
    smine: Vec<[u64; 2]>, // [location, amount]
    key: Scalar,
    keylocation: HashSet<u64>,
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
    alltagsever: Vec<CompressedRistretto>,
    txses: Vec<Vec<u8>>,
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
    is_user: bool,
    sent_onces: HashSet<Vec<u8>>,
    knownvalidators: HashMap<u64,NodeId>,
    newest: u64,
    rmems: HashMap<u64,OTAccount>,
    rname: Vec<u8>,
    moneyreset: Option<Vec<u8>>,
    sync_returnaddr: Option<NodeId>,
    sync_theirnum: u64,
    sync_lightning: bool,
    outs: Option<Vec<(Account, Scalar)>>,
    oldstk: Option<(Account, Vec<[u64;2]>, u64)>,
    cumtime: f64,
    blocktime: f64,
    lightning_yielder: bool,
    gui_timer: Instant,
}

impl KhoraNode {
    /// saves the important information like staker state and block number to a file: "myNode"
    fn save(&self) {
        if !self.moneyreset.is_some() && !self.oldstk.is_some() {
            let sn = SavedNode {
                save_history: self.save_history,
                me: self.me,
                mine: self.mine.clone(),
                smine: self.smine.clone(), // [location, amount]
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
                outer_view: self.outer.plumtree_node().all_push_peers().into_iter().collect(),
                rmems: self.rmems.clone(),
                rname: self.rname.clone(),
                moneyreset: self.moneyreset.clone(),
                oldstk: self.oldstk.clone(),
                cumtime: self.cumtime,
                blocktime: self.blocktime,
                lightning_yielder: self.lightning_yielder,
                is_validator: self.is_validator,
            }; // just redo initial conditions on the rest
            let mut sn = bincode::serialize(&sn).unwrap();
            let mut f = File::create("myNode").unwrap();
            f.write_all(&mut sn).unwrap();
        }
    }

    /// loads the node information from a file: "myNode"
    fn load(inner: Node<Vec<u8>>, outer: Node<Vec<u8>>, gui_sender: channel::Sender<Vec<u8>>, gui_reciever: mpsc::Receiver<Vec<u8>>) -> KhoraNode {
        let mut buf = Vec::<u8>::new();
        let mut f = File::open("myNode").unwrap();
        f.read_to_end(&mut buf).unwrap();

        let sn = bincode::deserialize::<SavedNode>(&buf).unwrap();

        // tries to get back all the friends you may have lost since turning off the app
        let mut outer = outer;
        outer.dm(vec![], &sn.outer_view, true);
        KhoraNode {
            inner,
            outer,
            gui_sender,
            gui_reciever,
            timekeeper: Instant::now() - Duration::from_secs(1),
            waitingforentrybool: true,
            waitingforleaderbool: false,
            waitingforleadertime: Instant::now(),
            waitingforentrytime: Instant::now(),
            usurpingtime: Instant::now(),
            txses: vec![], // if someone is not a leader for a really long time they'll have a wrongly long list of tx
            sigs: vec![],
            save_history: sn.save_history,
            me: sn.me,
            mine: sn.mine.clone(),
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
            bloom: BloomFile::from_keys(sn.bloom[0],sn.bloom[1]),
            bnum: sn.bnum,
            lastbnum: sn.lastbnum,
            height: sn.height,
            sheight: sn.sheight,
            alltagsever: sn.alltagsever.clone(),
            headshard: sn.headshard.clone(),
            is_validator: sn.is_validator,
            is_user: true,
            sent_onces: HashSet::new(), // maybe occasionally clear this or replace with vecdeq?
            knownvalidators: HashMap::new(),
            doneerly: Instant::now(),
            newest: 0u64,
            rmems: HashMap::new(),
            rname: vec![],
            moneyreset: sn.moneyreset,
            sync_returnaddr: None,
            sync_theirnum: 0u64,
            sync_lightning: false,
            outs: None,
            oldstk: sn.oldstk,
            cumtime: sn.cumtime,
            blocktime: sn.blocktime,
            lightning_yielder: sn.lightning_yielder,
            gui_timer: Instant::now(),
        }
    }

    /// reads a full block (by converting it to lightning then reading that)
    fn readblock(&mut self, lastblock: NextBlock, m: Vec<u8>) -> bool {
        let lastlightning = lastblock.tolightning();
        let l = bincode::serialize(&lastlightning).unwrap();
        self.readlightning(lastlightning,l,Some(m.clone()))
    }

    /// reads a lightning block and saves information when appropriate
    fn readlightning(&mut self, lastlightning: LightningSyncBlock, m: Vec<u8>, largeblock: Option<Vec<u8>>) -> bool {
        if lastlightning.bnum >= self.bnum {
            let com = self.comittee.par_iter().map(|x| x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>();
            if lastlightning.shards.len() == 0 {
                println!("Error in block verification: there is no shard");
                return false;
            }
            let v: bool;
            if (lastlightning.shards[0] as usize >= self.headshard) && (lastlightning.last_name == self.lastname) {
                if self.is_validator {
                    v = lastlightning.verify_multithread(&com[lastlightning.shards[0] as usize], &self.stkinfo).is_ok();
                } else {
                    v = lastlightning.verify(&com[lastlightning.shards[0] as usize], &self.stkinfo).is_ok();
                }
            } else {
                v = false;
            }
            if v  {
                // saves your current information BEFORE reading the new block. It's possible a leader is trying to cause a fork which can only be determined 1 block later based on what the comittee thinks is real
                self.save();

                // if you are one of the validators who leave this turn, it is your responcibility to send the block to the outside world
                if self.exitqueue[self.headshard].range(..REPLACERATE).map(|&x| self.comittee[self.headshard][x]).any(|x| self.keylocation.contains(&(x as u64))) {
                    if let Some(mut lastblock) = largeblock.clone() {
                        lastblock.push(3);
                        println!("-----------------------------------------------\nsending out the new block {}!\n-----------------------------------------------",lastlightning.bnum);
                        self.outer.broadcast_now(lastblock); /* broadcast the block to the outside world */
                    }

                }
                self.headshard = lastlightning.shards[0] as usize;

                println!("=========================================================\nyay!");

                self.overthrown.remove(&self.stkinfo[lastlightning.leader.pk as usize].0);
                if self.stkinfo[lastlightning.leader.pk as usize].0 != self.leader {
                    self.overthrown.insert(self.leader);
                }

                // if you're synicng, you just infer the empty blocks that no one saves
                for _ in self.bnum..lastlightning.bnum {
                    println!("I missed a block!");
                    let reward = reward(self.cumtime,self.blocktime);
                    self.cumtime += self.blocktime;
                    self.blocktime = blocktime(self.cumtime);

                    self.gui_sender.send(vec![!NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut self.smine, reward) as u8,1]).expect("there's a problem communicating to the gui!");
                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);

                    if self.save_history {
                        if !self.lightning_yielder {
                            NextBlock::save(&vec![]);
                        }
                        LightningSyncBlock::save(&vec![]);
                    }

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


                // calculate the reward for this block as a function of the current time and scan either the block or an empty block based on conditions
                let reward = reward(self.cumtime,self.blocktime);
                if !(lastlightning.info.txout.is_empty() && lastlightning.info.stkin.is_empty() && lastlightning.info.stkout.is_empty()) {
                    let mut guitruster = !lastlightning.scanstk(&self.me, &mut self.smine, &mut self.sheight, &self.comittee, reward, &self.stkinfo);
                    guitruster = !lastlightning.scan(&self.me, &mut self.mine, &mut self.height, &mut self.alltagsever) && guitruster;
                    self.gui_sender.send(vec![guitruster as u8,1]).expect("there's a problem communicating to the gui!");

                    if self.save_history {
                        println!("saving block...");
                        lastlightning.update_bloom(&mut self.bloom,&self.is_validator);
                        if !self.lightning_yielder {
                            NextBlock::save(&largeblock.unwrap()); // important! if you select to recieve full blocks you CAN NOT recieve with lightning blocks (because if you do youd miss full blocks)
                        }
                        LightningSyncBlock::save(&m);
                    }
                    self.keylocation = self.smine.iter().map(|x| x[0]).collect();
                    lastlightning.scan_as_noone(&mut self.stkinfo, &mut self.queue, &mut self.exitqueue, &mut self.comittee, reward, self.save_history);

                    self.lastbnum = self.bnum;
                    let mut hasher = Sha3_512::new();
                    hasher.update(m);
                    self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                } else {
                    self.gui_sender.send(vec![!NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut self.smine, reward) as u8,1]).expect("there's a problem communicating to the gui!");
                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);
                    if self.save_history {
                        if !self.lightning_yielder {
                            NextBlock::save(&vec![]);
                        }
                        LightningSyncBlock::save(&vec![]);
                    }
                }
                self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                self.newest = self.queue[self.headshard][0] as u64;
                for i in 0..self.comittee.len() {
                    select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                }
                self.bnum += 1;

                self.votes = self.votes.iter().zip(self.comittee[self.headshard].iter()).map(|(z,&x)| z + lastlightning.validators.iter().filter(|y| y.pk == x as u64).count() as i32).collect::<Vec<_>>();
                


                

                
                /* LEADER CHOSEN BY VOTES (off blockchain, says which comittee member they should send stuff to) */
                let abouttoleave = self.exitqueue[self.headshard].range(..EXIT_TIME).into_iter().map(|z| self.comittee[self.headshard][*z].clone()).collect::<HashSet<_>>();
                self.leader = self.stkinfo[*self.comittee[self.headshard].iter().zip(self.votes.iter()).max_by_key(|(x,&y)| {
                    if abouttoleave.contains(x) || self.overthrown.contains(&self.stkinfo[**x].0) {
                        i32::MIN
                    } else {
                        y
                    }
                }).unwrap().0].0;
                /* LEADER CHOSEN BY VOTES (off blockchain, says which comittee member they should send stuff to) */
                // which ip's are in the comittee
                self.knownvalidators = self.knownvalidators.iter().filter_map(|(&location,&node)| {
                    if lastlightning.info.stkout.contains(&location) {
                        None
                    } else {
                        let location = location - lastlightning.info.stkout.iter().map(|x| (*x < location) as u64).sum::<u64>();
                        Some((location,node))
                    }
                }).collect::<HashMap<_,_>>();
                self.knownvalidators = self.knownvalidators.iter().filter_map(|(&location,&node)| {
                    if self.queue[self.headshard].contains(&(location as usize)) || self.comittee[self.headshard].contains(&(location as usize)) {
                        Some((location,node))
                    } else {
                        None
                    }
                }).collect::<HashMap<_,_>>();


                // send info to the gui
                let mut mymoney = self.mine.iter().map(|x| self.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
                mymoney.extend(self.smine.iter().map(|x| x[1]).sum::<u64>().to_le_bytes());
                mymoney.push(0);
                println!("my money:\n---------------------------------\n{:?}",self.mine.iter().map(|x| self.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>());
                println!("my stake:\n---------------------------------\n{:?}",self.smine.iter().map(|x| x[1]).sum::<u64>().to_le_bytes());
                self.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui
                let mut thisbnum = self.bnum.to_le_bytes().to_vec();
                thisbnum.push(2);
                self.gui_sender.send(thisbnum).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui

                self.gui_sender.send(vec![self.keylocation.iter().any(|keylocation| self.comittee[self.headshard].contains(&(*keylocation as usize))) as u8,3]).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui
                println!("block {} name: {:?}",self.bnum, self.lastname);

                // delete the set of overthrone leaders sometimes to give them another chance
                if self.bnum % 128 == 0 {
                    self.overthrown = HashSet::new();
                }
                // if you save the history, the txses you know about matter; otherwise, they don't (becuase you're not involved in block creation)
                if self.save_history {
                    let s = self.stkinfo.borrow();
                    let bloom = self.bloom.borrow();
                    println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\nhad {} tx",self.txses.len());
                    self.txses = self.txses.iter().collect::<HashSet<_>>().into_iter().cloned().collect::<Vec<_>>();
                    self.txses.retain(|x| {
                        if let Ok(x) = bincode::deserialize::<PolynomialTransaction>(x) {
                            if x.inputs.last() == Some(&1) {
                                x.verifystk(s).is_ok()
                            } else {
                                x.tags.iter().all(|x| !bloom.contains(x.as_bytes())) && x.verify().is_ok()
                            }
                        } else {
                            false
                        }
                    });
                } else {
                    self.txses = vec![];
                }
                
                // runs any operations needed for the panic button to function
                self.send_panic_or_stop(&lastlightning, reward);

                // if you're lonely and on the comittee, you try to reconnect with the comittee (WARNING: DOES NOT HANDLE IF YOU HAVE FRIENDS BUT THEY ARE IGNORING YOU)
                if self.is_validator && self.inner.plumtree_node().all_push_peers().is_empty() {
                    for n in self.knownvalidators.iter() {
                        self.inner.join(n.1.with_id(1));
                    }
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
                println!("block reading process done!!!");












                // this section tells you if you're on the comittee or not
                // if you're on the comittee you need to pull on your inner node
                // if you're not you need to poll on your user node
                if self.keylocation.iter().all(|keylocation| !self.comittee[self.headshard].contains(&(*keylocation as usize)) ) { // if you're not in the comittee
                    self.is_user = true;
                    self.is_validator = false;
                } else { // if you're in the comittee
                    // println!("I'm in the comittee!");
                    self.is_user = false;
                    self.is_validator = true;
                }
                // if you're about to be in the comittee you need to take these actions
                self.keylocation.clone().iter().for_each(|keylocation| {
                    // announce yourself to the comittee because it's about to be your turn
                    if self.queue[self.headshard].range(0..REPLACERATE).any(|&x| x as u64 != *keylocation) {
                        self.is_user = true;
                        self.is_validator = true;


                        let message = bincode::serialize(self.inner.plumtree_node().id()).unwrap();
                        let mut evidence = Signature::sign_message(&self.key, &message, &keylocation);
                        evidence.push(118); // v
                        self.inner.dm_now(evidence,&self.knownvalidators.iter().filter_map(|(&location,&node)| {
                            if self.comittee[self.headshard].contains(&(location as usize)) && !(self.inner.plumtree_node().all_push_peers().contains(&node) || (node == self.inner.plumtree_node().id)) {
                                println!("(((((((((((((((((((((((((((((((((((((((((((((((dm'ing validators)))))))))))))))))))))))))))))))))))))))))))))))))))))");
                                Some(node)
                            } else {
                                None
                            }
                        }).collect::<Vec<_>>(), true);

                        let message = bincode::serialize(self.inner.plumtree_node().id()).unwrap();
                        if self.sent_onces.insert(message.clone().into_iter().chain(self.bnum.to_le_bytes().to_vec().into_iter()).collect::<Vec<_>>()) {
                            println!("broadcasting name!");
                            let mut evidence = Signature::sign_message(&self.key, &message, keylocation);
                            evidence.push(118); // v
                            self.outer.broadcast_now(evidence);
                        }
                    }
                });

                return true
            }
        }
        false
    }

    /// runs the operations needed for the panic button to work
    fn send_panic_or_stop(&mut self, lastlightning: &LightningSyncBlock, reward: f64) {
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
                    let y = oldstk.2/2u64.pow(BETA as u32) + 1;
                    for _ in 0..y {
                        let stkamnt = Scalar::from(oldstk.2/y);
                        outs.push((&self.me,stkamnt));
                    }
                    let tx = Transaction::spend_ring(&inps, &outs.iter().map(|x|(x.0,&x.1)).collect());
                    println!("about to verify!");
                    tx.verify().unwrap();
                    println!("finished to verify!");
                    let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                    loc.push(1);
                    let tx = tx.polyform(&loc); // push 0
                    if tx.verifystk(&self.stkinfo).is_ok() {
                        let mut txbin = bincode::serialize(&tx).unwrap();
                        self.txses.push(txbin.clone());
                        txbin.push(0);
                        self.outer.broadcast_now(txbin.clone());
                        self.inner.broadcast_now(txbin.clone());
                    }
                }
                if oldstkcheck {
                    self.oldstk = None;
                }
                if self.mine.len() > 0 && self.oldstk.is_some() {
                    self.moneyreset = None;
                }
                if let Some(x) = self.moneyreset.clone() {
                    self.outer.broadcast(x);
                }
            } else {
                self.moneyreset = None;
            }
        }
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
                if ((self.doneerly.elapsed().as_secs() > self.blocktime as u64) && (self.doneerly.elapsed() > self.timekeeper.elapsed())) || self.timekeeper.elapsed().as_secs() > self.blocktime as u64 {
                    self.waitingforentrybool = true;
                    self.waitingforleaderbool = false;
                    self.waitingforleadertime = Instant::now();
                    self.waitingforentrytime = Instant::now();
                    self.timekeeper = Instant::now();
                    self.doneerly = Instant::now();
                    self.usurpingtime = Instant::now();
                    println!("time thing happpened");
    
                    // if you are the newest member of the comittee you're responcible for choosing the tx that goes into the next block
                    if self.keylocation.contains(&(self.newest as u64)) {
                        let m = bincode::serialize(&self.txses).unwrap();
                        println!("_._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._.\nsending {} txses!",self.txses.len());
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
                        if mtype == 2 {print!("#{:?}", mtype);}
                        else {println!("# MESSAGE TYPE: {:?} FROM: {:?}", mtype,msg.id.node());}


                        if mtype == 1 /* the transactions you're supposed to filter and make a block for */ {
                            if let Some(who) = Signature::recieve_signed_message_nonced(&mut m, &self.stkinfo, &self.bnum) {
                                if (who == self.newest) || (self.stkinfo[who as usize].0 == self.leader) {
                                    if let Ok(m) = bincode::deserialize::<Vec<Vec<u8>>>(&m) {
                                        let m = m.into_par_iter().filter_map(|x|
                                            if let Ok(x) = bincode::deserialize(&x) {
                                                Some(x)
                                            } else {
                                                None
                                            }
                                        ).collect::<Vec<PolynomialTransaction>>();

                                        for keylocation in &self.keylocation {
                                            let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, &m, &(self.headshard as u16), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo);
                                            println!("{:?}",m.txs.len());
                                            let mut m = bincode::serialize(&m).unwrap();
                                            m.push(2);
                                            for _ in self.comittee[self.headshard].iter().filter(|&x|*x as u64 == *keylocation).collect::<Vec<_>>() {
                                                self.inner.broadcast(m.clone());
                                            }
                                        }
                                        self.waitingforentrybool = false;
                                        self.waitingforleaderbool = true;
                                        self.waitingforleadertime = Instant::now();
                                        self.inner.handle_gossip_now(fullmsg, true);
                                    } else {
                                        self.inner.handle_gossip_now(fullmsg, false);
                                    }
                                } else {
                                    self.inner.handle_gossip_now(fullmsg, false);
                                }
                            }
                        } else if mtype == 2 /* the signatures you're supposed to process as the leader */ {
                            if let Ok(sig) = bincode::deserialize(&m) {
                                self.sigs.push(sig);
                                self.inner.handle_gossip_now(fullmsg, true);
                            } else {
                                self.inner.handle_gossip_now(fullmsg, false);
                            }
                        } else if mtype == 3 /* the full block that was made */ {
                            if let Ok(lastblock) = bincode::deserialize::<NextBlock>(&m) {
                                if self.readblock(lastblock, m) {
                                    self.inner.handle_gossip_now(fullmsg, true);
                                } else {
                                    self.inner.handle_gossip_now(fullmsg, false);
                                }
                            } else {
                                self.inner.handle_gossip_now(fullmsg, false);
                            }
                        } else if mtype == 118 /* v */ /* evidence someone announced is a validator */ {
                            if let Some(who) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                                if let Ok(m) = bincode::deserialize::<NodeId>(&m) {
                                    if self.queue[self.headshard].contains(&(who as usize)) {
                                        self.inner.plumtree_node.lazy_push_peers.insert(m);
                                    }
                                }
                            }
                            self.inner.handle_gossip_now(fullmsg, false);
                        } else /* spam that you choose not to propegate */ {
                            // self.inner.kill(&fullmsg.sender);
                            self.inner.handle_gossip_now(fullmsg, false);
                        }
                    }
                    did_something = true;
                }
                // if the leader didn't show up, overthrow them
                if (self.waitingforleadertime.elapsed().as_secs() > (0.5*self.blocktime) as u64) && self.waitingforleaderbool {
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
        
                            println!("made a block with {} transactions!",lastblock.txs.len());
    
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

                            println!("failed to make block right now");
                        }
        
                    }
                }

                // if the entry person didn't show up start trying to make an empty block
                if self.waitingforentrybool && (self.waitingforentrytime.elapsed().as_secs() > (0.66*self.blocktime) as u64) {
                    self.waitingforentrybool = false;
                    for keylocation in &self.keylocation {
                        let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, &vec![], &(self.headshard as u16), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo);
                        println!("trying to make an empty block...");
                        let mut m = bincode::serialize(&m).unwrap();
                        m.push(2);
                        if self.comittee[self.headshard].contains(&(*keylocation as usize)) {
                            println!("I'm sending a MESSAGE TYPE 4 to {:?}",self.inner.plumtree_node().all_push_peers());
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
            if self.is_user {
                // if you're syncing someone sync a few more blocks every loop time
                if let Some(addr) = self.sync_returnaddr {
                    while self.sync_theirnum <= self.bnum {
                        println!("checking for file location for {}...",self.sync_theirnum);
                        if self.sync_lightning {
                            if let Ok(mut x) = LightningSyncBlock::read(&self.sync_theirnum) {
                                x.push(3);
                                self.outer.dm(x,&vec![addr],false);
                                self.sync_theirnum += 1;
                                break;
                            } else {
                                self.sync_theirnum += 1;
                            }
                        } else {
                            if let Ok(mut x) = NextBlock::read(&self.sync_theirnum) {
                                x.push(3);
                                self.outer.dm(x,&vec![addr],false);
                                self.sync_theirnum += 1;
                                break;
                            } else {
                                self.sync_theirnum += 1;
                            }
                        }
                    }
                }


                // recieved a message as a non comittee member
                while let Async::Ready(Some(fullmsg)) = track_try_unwrap!(self.outer.poll()) {
                    let msg = fullmsg.message.clone();
                    let mut m = msg.payload.to_vec();
                    if let Some(mtype) = m.pop() {
                        println!("# MESSAGE TYPE: {:?}", mtype);


                        if mtype == 0 /* transaction someone wants to make */ {
                            // to avoid spam
                            let m = m[..std::cmp::min(m.len(),100_000)].to_vec();
                            if let Ok(t) = bincode::deserialize::<PolynomialTransaction>(&m) {
                                if self.save_history {
                                    let ok = {
                                        if t.inputs.last() == Some(&1) {
                                            t.verifystk(&self.stkinfo).is_ok()
                                        } else {
                                            let bloom = self.bloom.borrow();
                                            t.tags.iter().all(|y| !bloom.contains(y.as_bytes())) && t.verify().is_ok()
                                        }
                                    };
                                    if ok {
                                        self.txses.push(m);
                                        print!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\ngot a tx, now at {}!",self.txses.len());
                                        self.outer.handle_gossip_now(fullmsg, true);
                                    }
                                } else {
                                    self.outer.handle_gossip_now(fullmsg, true);
                                }
                            } else {
                                self.outer.handle_gossip_now(fullmsg, false);
                            }
                        } else if mtype == 3 /* recieved a full block */ {
                            if let Ok(lastblock) = bincode::deserialize::<NextBlock>(&m) {
                                let s = self.readblock(lastblock, m);
                                self.outer.handle_gossip_now(fullmsg, s);
                            } else {
                                self.outer.handle_gossip_now(fullmsg, false);
                            }
                        } else if mtype == 60 /* < */ { // redo sync request
                            let mut mynum = self.bnum.to_le_bytes().to_vec();
                            if self.lightning_yielder { // lightning users don't ask for full blocks
                                mynum.push(108); //l
                            } else {
                                mynum.push(102); //f
                            }
                            mynum.push(121);
                            if let Ok(x) = bincode::deserialize(&m) {
                                self.outer.dm(mynum, &[x], false);
                            } else {
                                let mut friend = self.outer.plumtree_node().all_push_peers();
                                friend.remove(&msg.id.node());
                                friend.remove(self.outer.plumtree_node().id());
                                let friend = friend.into_iter().collect::<Vec<_>>();
                                if let Some(friend) = friend.choose(&mut rand::thread_rng()) {
                                    println!("asking for help from {:?}",friend);
                                    self.outer.dm(mynum, &[*friend], false);
                                } else {
                                    println!("you're isolated");
                                }
                            }
                        } else if mtype == 97 /* a */ {
                            self.outer.plumtree_node.lazy_push_peers.insert(fullmsg.sender);
                        } else if mtype == 108 /* l */ { // a lightning block
                            if self.lightning_yielder {
                                if let Ok(lastblock) = bincode::deserialize::<LightningSyncBlock>(&m) {
                                    self.readlightning(lastblock, m, None); // that whole thing with 3 and 8 makes it super unlikely to get more blocks (expecially for my small thing?)
                                }
                            }
                        } else if mtype == 113 /* q */ { // they just sent you a ring member
                            self.rmems.insert(u64::from_le_bytes(m[64..72].try_into().unwrap()),History::read_raw(&m));
                        } else if mtype == 114 /* r */ { // answer their ring question
                            let mut y = m[..8].to_vec();
                            let mut x = History::get_raw(&u64::from_le_bytes(y.clone().try_into().unwrap())).to_vec();
                            x.append(&mut y);
                            x.push(113);
                            self.outer.dm(x,&vec![msg.id.node()],false);
                        } else if mtype == 118 /* v */ { // someone announcing they're about to be in the comittee
                            if let Some(who) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                                if let Ok(m) = bincode::deserialize::<NodeId>(&m) {
                                    if self.queue[self.headshard].contains(&(who as usize)) {
                                        self.knownvalidators.insert(who,m);
                                        self.outer.handle_gossip_now(fullmsg, true);
                                    } else {
                                        self.outer.handle_gossip_now(fullmsg, false);
                                    }
                                } else {
                                    self.outer.handle_gossip_now(fullmsg, false);
                                }
                            } else {
                                self.outer.handle_gossip_now(fullmsg, false);
                            }
                        } else if mtype == 121 /* y */ { // someone sent a sync request
                            let mut i_cant_do_this = true;
                            if self.save_history {
                                if self.sync_returnaddr.is_none() {
                                    if let Some(theyfast) = m.pop() {
                                        if let Ok(m) = m.try_into() {
                                            if theyfast == 108 {
                                                self.sync_lightning = true;
                                                self.sync_returnaddr = Some(msg.id.node());
                                                self.sync_theirnum = u64::from_le_bytes(m);
                                                i_cant_do_this = false;
                                            } else {
                                                if !self.lightning_yielder {
                                                    self.sync_lightning = false;
                                                    self.sync_returnaddr = Some(msg.id.node());
                                                    self.sync_theirnum = u64::from_le_bytes(m);
                                                    i_cant_do_this = false;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if msg.id.node() != *self.outer.plumtree_node().id() && i_cant_do_this {
                                let mut friend = self.outer.plumtree_node().all_push_peers().into_iter().collect::<Vec<_>>();
                                friend.retain(|&x| x != fullmsg.sender);
                                if let Some(friend) = friend.choose(&mut rand::thread_rng()) {
                                    println!("asking for help from {:?}",friend);
                                    let mut m = bincode::serialize(friend).unwrap();
                                    m.push(60);
                                    self.outer.dm(m, &[*friend], false);
                                } else {
                                    self.outer.dm(vec![60], &[msg.id.node()], false);
                                    println!("you're isolated");
                                }
                            }
                        } else /* spam */ {
                            // self.outer.kill(&fullmsg.sender);
                            self.outer.handle_gossip_now(fullmsg, false);
                        }
                    }
                }
                did_something = true;
            }













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
            // handles ring formation for people who don't have access to the history of the blockchain
            if !self.save_history {
                if let Some(outs) = self.outs.clone() {
                    let ring = recieve_ring(&self.rname).expect("shouldn't fail");
                    let mut rlring = ring.iter().map(|x| self.rmems[x].clone()).collect::<Vec<OTAccount>>();
                    rlring.iter_mut().for_each(|x|if let Ok(y)=self.me.receive_ot(&x) {*x = y;});
                    let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                    if tx.verify().is_ok() {
                        let tx = tx.polyform(&self.rname);
                        // tx.verify().unwrap(); // as a user you won't be able to check this
                        let mut txbin = bincode::serialize(&tx).unwrap();
                        txbin.push(0);
                        let needtosend = (txbin,self.mine.iter().map(|x| *x.0).collect::<Vec<_>>());
                        self.outer.broadcast_now(needtosend.0.clone());
                        println!("transaction made!");
                        self.outs = None;
                    } else {
                        println!("you can't make that transaction, user!");
                    }
                }
            }
            // interacting with the gui
            while let Async::Ready(Some(mut m)) = self.gui_reciever.poll().expect("Never fails") {
                println!("got message from gui!\n{}",String::from_utf8_lossy(&m));
                if let Some(istx) = m.pop() {
                    let mut validtx = true;
                    if istx == 33 /* ! */ { // a transaction
                        let txtype = m.pop().unwrap();
                        let mut outs = vec![];
                        while m.len() > 0 {
                            let mut pks = vec![];
                            for _ in 0..3 { // read the pk address
                                if m.len() >= 64 {
                                    let h1 = m.drain(..32).collect::<Vec<_>>().iter().map(|x| (x-97)).collect::<Vec<_>>();
                                    let h2 = m.drain(..32).collect::<Vec<_>>().iter().map(|x| (x-97)*16).collect::<Vec<_>>();
                                    if let Ok(p) = h1.into_iter().zip(h2).map(|(x,y)|x+y).collect::<Vec<u8>>().try_into() {
                                        pks.push(CompressedRistretto(p));
                                    } else {
                                        pks.push(RISTRETTO_BASEPOINT_COMPRESSED);
                                        validtx = false;
                                    }
                                } else {
                                    pks.push(RISTRETTO_BASEPOINT_COMPRESSED);
                                    validtx = false;
                                }
                            }
                            if m.len() >= 8 {
                                if let Ok(x) = m.drain(..8).collect::<Vec<_>>().try_into() {
                                    let x = u64::from_le_bytes(x);
                                    println!("amounts {:?}",x);
                                    let y = x/2u64.pow(BETA as u32) + 1;
                                    println!("need to split this up into {} txses!",y);
                                    let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                                    for _ in 0..y {
                                        let amnt = Scalar::from(x/y);
                                        outs.push((recv,amnt));
                                    }
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
                            let (loc, acc): (Vec<u64>,Vec<OTAccount>) = self.mine.iter().map(|x|(x.0,x.1.clone())).unzip();

                            // if you need help with ring generation
                            if !self.save_history {
                                if self.mine.len() > 0 {                                
                                    let (loc, acc): (Vec<u64>,Vec<OTAccount>) = self.mine.iter().map(|x|(*x.0,x.1.clone())).unzip();
                
                                    println!("loc: {:?}",loc);
                                    println!("height: {}",self.height);
                                    for (i,j) in loc.iter().zip(acc) {
                                        println!("i: {}, j.pk: {:?}",i,j.pk.compress());
                                        self.rmems.insert(*i,j);
                                    }
                                    
                                    self.rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + 4), &self.height);
                                    let ring = recieve_ring(&self.rname).expect("shouldn't fail");
                                    let ring = ring.into_iter().filter(|x| loc.iter().all(|y|x!=y)).collect::<Vec<_>>();
                                    println!("ring:----------------------------------\n{:?}",ring);
                                    for r in ring.iter() {
                                        let mut r = r.to_le_bytes().to_vec();
                                        r.push(114u8);
                                        self.outer.dm(r,&self.outer.plumtree_node().all_push_peers(),false);
                                    }

                                    self.outs = Some(outs);
                                }
                                txbin = vec![];
                            } else {
                                let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + 4), &self.height);
                                let ring = recieve_ring(&rname).expect("shouldn't fail");
                                println!("ring: {:?}",ring);
                                println!("mine: {:?}",acc.iter().map(|x|x.pk.compress()).collect::<Vec<_>>());
                                // println!("ring: {:?}",ring.iter().map(|x|OTAccount::summon_ota(&History::get(&x)).pk.compress()).collect::<Vec<_>>());
                                let mut rlring = ring.into_iter().map(|x| {
                                    let x = OTAccount::summon_ota(&History::get(&x));
                                    if acc.iter().all(|a| a.pk != x.pk) {
                                        println!("not mine!");
                                        x
                                    } else {
                                        println!("mine!");
                                        acc.iter().filter(|a| a.pk == x.pk).collect::<Vec<_>>()[0].to_owned()
                                    }
                                }).collect::<Vec<OTAccount>>();
                                println!("ring len: {:?}",rlring.len());
                                let me = self.me;
                                rlring.iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x) {*x = y;});
                                let tx = Transaction::spend_ring(&rlring, &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                                let tx = tx.polyform(&rname);
                                if tx.verify().is_ok() {
                                    txbin = bincode::serialize(&tx).unwrap();
                                    println!("transaction made!");
                                } else {
                                    txbin = vec![];
                                    println!("you can't make that transaction!");
                                }
                            }
                        } else if txtype == 63 /* ? */ { // transaction should be spent with staked money
                            let (loc, amnt): (Vec<u64>,Vec<u64>) = self.smine.iter().map(|x|(x[0] as u64,x[1].clone())).unzip();
                            let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                            let tx = Transaction::spend_ring(&inps, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                            println!("about to verify!");
                            tx.verify().unwrap();
                            println!("finished to verify!");
                            let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                            loc.push(1);
                            let tx = tx.polyform(&loc); // push 0
                            if tx.verifystk(&self.stkinfo).is_ok() {
                                txbin = bincode::serialize(&tx).unwrap();
                                println!("sending tx!");
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
                            self.txses.push(txbin.clone());
                            txbin.push(0);
                            self.outer.broadcast_now(txbin);
                            println!("transaction broadcasted");
                        } else {
                            println!("transaction not made right now");
                        }
                    } else if istx == u8::MAX /* panic button */ {
                        
                        let amnt = u64::from_le_bytes(m.drain(..8).collect::<Vec<_>>().try_into().unwrap());
                        // let amnt = Scalar::from(amnt);
                        let mut stkamnt = u64::from_le_bytes(m.drain(..8).collect::<Vec<_>>().try_into().unwrap());
                        // let mut stkamnt = Scalar::from(stkamnt);
                        if stkamnt == amnt {
                            stkamnt -= 1;
                        }
                        let newacc = Account::new(&format!("{}",String::from_utf8_lossy(&m)));

                        // send unstaked money
                        if self.mine.len() > 0 {
                            let (loc, _acc): (Vec<u64>,Vec<OTAccount>) = self.mine.iter().map(|x|(x.0,x.1.clone())).unzip();

                            println!("remembered owned accounts");
                            let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16), &self.height);
                            let ring = recieve_ring(&rname).expect("shouldn't fail");

                            println!("made rings");
                            /* you don't use a ring for panics (the ring is just your own accounts) */ 
                            let mut rlring = ring.iter().map(|&x| self.mine.iter().filter(|(&y,_)| y == x).collect::<Vec<_>>()[0].1.clone()).collect::<Vec<OTAccount>>();
                            let me = self.me;
                            rlring.iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x) {*x = y;});
                            
                            let mut outs = vec![];
                            let y = amnt/2u64.pow(BETA as u32) + 1;
                            for _ in 0..y {
                                let amnt = Scalar::from(amnt/y);
                                outs.push((&newacc,amnt));
                            }
                            let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x| (x.0,&x.1)).collect());

                            println!("{:?}",rlring.iter().map(|x| x.com.amount).collect::<Vec<_>>());
                            println!("{:?}",amnt);
                            if tx.verify().is_ok() {
                                let tx = tx.polyform(&rname);
                                if self.save_history {
                                    tx.verify().unwrap(); // as a user you won't be able to check this
                                }
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                self.txses.push(txbin.clone());
                                txbin.push(0);
                                self.outer.broadcast_now(txbin.clone());
                                self.moneyreset = Some(txbin);
                                println!("transaction made!");
                            } else {
                                println!("you can't make that transaction, user!");
                            }
                        }

                        // send staked money
                        if self.smine.len() > 0 {
                            let (loc, amnt): (Vec<u64>,Vec<u64>) = self.smine.iter().map(|x|(x[0],x[1])).unzip();
                            let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();


                            let mut outs = vec![];
                            let y = stkamnt/2u64.pow(BETA as u32) + 1;
                            for _ in 0..y {
                                let stkamnt = Scalar::from(stkamnt/y);
                                outs.push((&newacc,stkamnt));
                            }
                            let tx = Transaction::spend_ring(&inps, &outs.iter().map(|x| (x.0,&x.1)).collect());
                            println!("about to verify!");
                            tx.verify().unwrap();
                            println!("finished to verify!");
                            let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                            loc.push(1);
                            let tx = tx.polyform(&loc); // push 0
                            if tx.verifystk(&self.stkinfo).is_ok() {
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                self.txses.push(txbin.clone());
                                txbin.push(0);
                                self.outer.broadcast_now(txbin.clone());
                                self.oldstk = Some((self.me.clone(),self.smine.clone(),stkamnt));
                                println!("sending tx!");
                            } else {
                                println!("you can't make that transaction!");
                            }
                        }


                        self.mine = HashMap::new();
                        self.smine = vec![];
                        self.me = newacc;
                        self.key = self.me.stake_acc().receive_ot(&self.me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap().sk.unwrap();
                        self.keylocation = HashSet::new();
                        let mut m1 = self.me.name().as_bytes().to_vec();
                        m1.extend([0,u8::MAX]);
                        let mut m2 = self.me.stake_acc().name().as_bytes().to_vec();
                        m2.extend([1,u8::MAX]);
                        let mut m3 = self.me.sk.as_bytes().to_vec();
                        m3.extend([2,u8::MAX]);
                        let mut m4 = self.me.vsk.as_bytes().to_vec();
                        m4.extend([3,u8::MAX]);
                        let mut m5 = self.me.ask.as_bytes().to_vec();
                        m5.extend([4,u8::MAX]);
                        self.gui_sender.send(m1).expect("should be working");
                        self.gui_sender.send(m2).expect("should be working");
                        self.gui_sender.send(m3).expect("should be working");
                        self.gui_sender.send(m4).expect("should be working");
                        self.gui_sender.send(m5).expect("should be working");

                    } else if istx == 121 /* y */ { // you clicked sync
                        let mut mynum = self.bnum.to_le_bytes().to_vec();
                        if self.lightning_yielder {
                            mynum.push(108); //l
                        } else {
                            mynum.push(102); //f
                        }
                        mynum.push(121);
                        let mut friend = self.outer.plumtree_node().all_push_peers();
                        friend.remove(self.outer.plumtree_node().id());
                        println!("{:?}",friend);
                        let friend = friend.into_iter().collect::<Vec<_>>();
                        println!("friends: {:?}",friend);
                        let mut gm = (friend.len() as u16).to_le_bytes().to_vec();
                        gm.push(4);
                        self.gui_sender.send(gm).expect("should be working");
                        if let Some(friend) = friend.choose(&mut rand::thread_rng()) {
                            println!("asking for help from {:?}",friend);
                            self.outer.dm(mynum, &[*friend], false);
                        } else {
                            println!("you're isolated");
                        }
                    } else if istx == 42 /* * */ { // entry address
                        let m = format!("{}:{}",String::from_utf8_lossy(&m),DEFAULT_PORT);
                        println!("{}",m);
                        if let Ok(socket) = m.parse() {
                            println!("it's a socket");
                            self.outer.dm(vec![97],&[NodeId::new(socket, LocalNodeId::new(0))],true);
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
                    }
                }
            }












        }
        Ok(Async::NotReady)
    }
}
