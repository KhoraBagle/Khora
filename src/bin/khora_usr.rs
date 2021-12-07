#[macro_use]
// #[allow(unreachable_code)]
extern crate trackable;

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_COMPRESSED;
use fibers::sync::mpsc;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use khora::seal::BETA;
use natpmp::Natpmp;
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use rand::prelude::SliceRandom;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
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
use khora::validation::*;
use khora::ringmaker::*;
use serde::{Serialize, Deserialize};
use khora::validation::{NUMBER_OF_VALIDATORS, QUEUE_LENGTH};




/// amount of seconds to wait before initiating shard takeover
const USURP_TIME: u64 = 3600;
/// the outsider port
const OUTSIDER_PORT: u16 = 8335;
/// total money ever produced
const TOTAL_KHORA: f64 = 1.0e16;
/// the number of nodes to send each message to
const TRANSACTION_SEND_TO: usize = 1;
/// the number of nodes to send each message to
const SYNC_SEND_TO: usize = 1;
/// the number of nodes to send each message to
const RING_SEND_TO: usize = 3;
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

    


    
    // this is the number of shards they keep track of
    let max_shards = 64usize; /* this if for testing purposes... there IS NO MAX SHARDS */
    



    // println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);
    let executor = track_any_err!(ThreadPoolExecutor::new())?;

    let (ui_sender, mut urecv) = mpsc::channel();
    let (usend, ui_reciever) = channel::unbounded();



    
    // the myUsr file only exists if you already have an account made
    let setup = !Path::new("myUsr").exists();
    if setup {
        // everyone agrees this person starts with 1 khora token
        let person0 = CompressedRistretto([46, 235, 227, 188, 55, 53, 9, 126, 167, 207, 202, 101, 150, 150, 172, 207, 209, 208, 211, 52, 47, 206, 19, 115, 199, 189, 202, 10, 56, 220, 138, 55]);
        let initial_history = vec![(person0,1u64)];

        std::thread::spawn(move || {
            let pswrd: Vec<u8>;
            let will_stk: bool;
            loop {
                if let Async::Ready(Some(m)) = urecv.poll().expect("Shouldn't fail") {
                    pswrd = m;
                    break
                }
            }
            loop {
                if let Async::Ready(Some(m)) = urecv.poll().expect("Shouldn't fail") {
                    will_stk = m[0] == 0;
                    break
                }
            }
            println!("{:?}",pswrd);
            let me = Account::new(&pswrd);
            let validator = me.stake_acc().receive_ot(&me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap(); //make a new account
            let key = validator.sk.unwrap();
            if will_stk {
                History::initialize();
            }
    
    
            // creates the node with the specified conditions then saves it to be used from now on
            let node = KhoraNode {
                view: vec![],
                save_history: will_stk,
                me,
                mine: HashMap::new(),
                key,
                stkinfo: initial_history.clone(),
                queue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                exitqueue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                comittee: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<Vec<usize>>()).collect::<Vec<_>>(),
                lastname: Scalar::one().as_bytes().to_vec(),
                bnum: 0u64,
                lastbnum: 0u64,
                height: 0u64,
                sheight: initial_history.len() as u64,
                alltagsever: vec![],
                headshard: 0,
                usurpingtime: Instant::now(),
                knownnodes: HashMap::new(),
                rmems: HashMap::new(),
                rname: vec![],
                gui_sender: usend.clone(),
                gui_reciever: mpsc::channel().1,
                moneyreset: None,
                outs: None,
                cumtime: 0f64,
                blocktime: blocktime(0.0),
                ringsize: 5,
                gui_timer: Instant::now(),
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


            let node = KhoraNode::load(usend, urecv);
            executor.spawn(node);
            println!("about to run node executer!");
            track_any_err!(executor.run()).unwrap();
        });
        // creates the setup screen (sets the values used in the loops and sets some gui options)
        let app = gui::user::KhoraUserGUI::new(
            ui_reciever,
            ui_sender,
            "".to_string(),
            "".to_string(),
            vec![],
            vec![],
            vec![],
            true,
        );
        let mut native_options = eframe::NativeOptions::default();
        eframe::run_native(Box::new(app), native_options);
    } else {
        let node = KhoraNode::load(usend, urecv);
        let mut mymoney = node.mine.iter().map(|x| node.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
        mymoney.push(0);
        node.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui
        node.save();

        let app = gui::user::KhoraUserGUI::new(
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
            executor.spawn(node);
            track_any_err!(executor.run()).unwrap();
        });
        eframe::run_native(Box::new(app), native_options);
    }
    







}

#[derive(Clone, Serialize, Deserialize, Debug)]
/// the information that you save to a file when the app is off (not including gui information like saved friends)
struct SavedNode {
    view: Vec<SocketAddr>,
    save_history: bool, //just testing. in real code this is true; but i need to pretend to be different people on the same computer
    me: Account,
    mine: HashMap<u64, OTAccount>,
    key: Scalar,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bnum: u64,
    lastbnum: u64,
    height: u64,
    sheight: u64,
    alltagsever: Vec<CompressedRistretto>,
    headshard: usize,
    rname: Vec<u8>,
    moneyreset: Option<Vec<u8>>,
    cumtime: f64,
    blocktime: f64,
    ringsize: u8,
    knownnodes: HashMap<u64,SocketAddr>,
}

/// the node used to run all the networking
struct KhoraNode {
    gui_sender: channel::Sender<Vec<u8>>,
    gui_reciever: mpsc::Receiver<Vec<u8>>,
    view: Vec<SocketAddr>,
    save_history: bool, // do i really want this? yes?
    me: Account,
    mine: HashMap<u64, OTAccount>,
    key: Scalar,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bnum: u64,
    lastbnum: u64,
    height: u64,
    sheight: u64,
    alltagsever: Vec<CompressedRistretto>,
    headshard: usize,
    usurpingtime: Instant,
    knownnodes: HashMap<u64,SocketAddr>,
    rmems: HashMap<u64,OTAccount>,
    rname: Vec<u8>,
    moneyreset: Option<Vec<u8>>,
    outs: Option<Vec<(Account, Scalar)>>,
    cumtime: f64,
    blocktime: f64,
    gui_timer: Instant,
    ringsize: u8,
}

impl KhoraNode {
    /// saves the important information like staker state and block number to a file: "myUsr"
    fn save(&self) {
        if self.moneyreset.is_none() {
            let sn = SavedNode {
                view: self.view.clone(),
                save_history: self.save_history,
                me: self.me,
                mine: self.mine.clone(),
                key: self.key,
                stkinfo: self.stkinfo.clone(),
                queue: self.queue.clone(),
                exitqueue: self.exitqueue.clone(),
                comittee: self.comittee.clone(),
                lastname: self.lastname.clone(),
                bnum: self.bnum,
                lastbnum: self.lastbnum,
                height: self.height,
                sheight: self.sheight,
                alltagsever: self.alltagsever.clone(),
                headshard: self.headshard.clone(),
                rname: self.rname.clone(),
                moneyreset: self.moneyreset.clone(),
                cumtime: self.cumtime,
                blocktime: self.blocktime,
                ringsize: self.ringsize,
                knownnodes: self.knownnodes.clone(),
            }; // just redo initial conditions on the rest
            let mut sn = bincode::serialize(&sn).unwrap();
            let mut f = File::create("myUsr").unwrap();
            f.write_all(&mut sn).unwrap();
        }
    }

    /// loads the node information from a file: "myUsr"
    fn load(gui_sender: channel::Sender<Vec<u8>>, gui_reciever: mpsc::Receiver<Vec<u8>>) -> KhoraNode {
        let mut buf = Vec::<u8>::new();
        let mut f = File::open("myUsr").unwrap();
        f.read_to_end(&mut buf).unwrap();

        let sn = bincode::deserialize::<SavedNode>(&buf).unwrap();

        KhoraNode {
            gui_sender,
            gui_reciever,
            view: sn.view.clone(),
            usurpingtime: Instant::now(),
            save_history: sn.save_history,
            me: sn.me,
            mine: sn.mine.clone(),
            key: sn.key,
            stkinfo: sn.stkinfo.clone(),
            queue: sn.queue.clone(),
            exitqueue: sn.exitqueue.clone(),
            comittee: sn.comittee.clone(),
            lastname: sn.lastname.clone(),
            bnum: sn.bnum,
            lastbnum: sn.lastbnum,
            height: sn.height,
            sheight: sn.sheight,
            alltagsever: sn.alltagsever.clone(),
            headshard: sn.headshard.clone(),
            knownnodes: sn.knownnodes.clone(),
            rmems: HashMap::new(),
            rname: vec![],
            moneyreset: sn.moneyreset,
            outs: None,
            cumtime: sn.cumtime,
            blocktime: sn.blocktime,
            gui_timer: Instant::now(),
            ringsize: sn.ringsize,
        }
    }

    /// reads a lightning block and saves information when appropriate and returns if you accepted the block
    fn readlightning(&mut self, lastlightning: LightningSyncBlock, m: Vec<u8>) -> bool {
        if lastlightning.bnum >= self.bnum {
            let com = self.comittee.par_iter().map(|x| x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>();
            if lastlightning.shards.len() == 0 {
                println!("Error in block verification: there is no shard");
                return false;
            }
            let v: bool;
            if (lastlightning.shards[0] as usize >= self.headshard) && (lastlightning.last_name == self.lastname) {
                v = lastlightning.verify(&com[lastlightning.shards[0] as usize], &self.stkinfo).is_ok();
            } else {
                v = false;
            }
            if v  {
                // saves your current information BEFORE reading the new block. It's possible a leader is trying to cause a fork which can only be determined 1 block later based on what the comittee thinks is real
                self.save();

                self.headshard = lastlightning.shards[0] as usize;

                println!("=========================================================\nyay!");


                // if you're synicng, you just infer the empty blocks that no one saves
                for _ in self.bnum..lastlightning.bnum {
                    println!("I missed a block!");
                    let reward = reward(self.cumtime,self.blocktime);
                    self.cumtime += self.blocktime;
                    self.blocktime = blocktime(self.cumtime);

                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);


                    for i in 0..self.comittee.len() {
                        select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                    }
                    self.bnum += 1;
                }


                // calculate the reward for this block as a function of the current time and scan either the block or an empty block based on conditions
                let reward = reward(self.cumtime,self.blocktime);
                if !(lastlightning.info.txout.is_empty() && lastlightning.info.stkin.is_empty() && lastlightning.info.stkout.is_empty()) {
                    lastlightning.scanstk(&self.me, &mut vec![], &mut self.sheight, &self.comittee, reward, &self.stkinfo);
                    let guitruster = lastlightning.scan(&self.me, &mut self.mine, &mut self.height, &mut self.alltagsever);
                    self.gui_sender.send(vec![!guitruster as u8,1]).expect("there's a problem communicating to the gui!");
                    if guitruster {
                        let mut mymoney = self.mine.iter().map(|x| self.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
                        mymoney.push(0);
                        println!("my money:\n---------------------------------\n{:?}",self.mine.iter().map(|x| self.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>());
                        self.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui    
                    }
                    lastlightning.scan_as_noone(&mut self.stkinfo, &mut self.queue, &mut self.exitqueue, &mut self.comittee, reward, self.save_history);

                    self.lastbnum = self.bnum;
                    let mut hasher = Sha3_512::new();
                    hasher.update(m);
                    self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                } else {
                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);
                }
                for i in 0..self.comittee.len() {
                    select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                }
                self.bnum += 1;
                

                
                // which ip's still have money
                self.knownnodes = self.knownnodes.iter().filter_map(|(&location,&node)| {
                    if lastlightning.info.stkout.contains(&location) {
                        None
                    } else {
                        let location = location - lastlightning.info.stkout.iter().map(|x| (*x < location) as u64).sum::<u64>();
                        Some((location,node))
                    }
                }).collect::<HashMap<_,_>>();


                // send info to the gui
                let mut thisbnum = self.bnum.to_le_bytes().to_vec();
                thisbnum.push(2);
                self.gui_sender.send(thisbnum).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui

                println!("block {} name: {:?}",self.bnum, self.lastname);

                
                // runs any operations needed for the panic button to function
                self.send_panic_or_stop(&lastlightning, reward);



                self.cumtime += self.blocktime;
                self.blocktime = blocktime(self.cumtime);

                self.gui_sender.send(vec![self.blocktime as u8,128]).expect("something's wrong with the communication to the gui");

                self.usurpingtime = Instant::now();
                println!("block reading process done!!!");













                return true
            }
        }
        false
    }

    /// runs the operations needed for the panic button to work
    fn send_panic_or_stop(&mut self, lastlightning: &LightningSyncBlock, reward: f64) {
        if self.moneyreset.is_some() {
            if self.mine.len() < 1 {
                self.send_message(self.moneyreset.clone().expect("just checked that this worked 2 lines ago"),TRANSACTION_SEND_TO);
            } else {
                self.moneyreset = None;
            }
        }
    }

    /// returns the responces of each person you sent it to and deletes those who are dead from the view
    fn send_message(&mut self, message: Vec<u8>, recipients: usize) -> Vec<Vec<u8>> {

        let mut rng = &mut rand::thread_rng();
        let v = self.view.choose_multiple(&mut rng, recipients).cloned().collect::<Vec<_>>();
        
        let dead = Arc::new(Mutex::new(HashSet::<SocketAddr>::new()));
        let responces = v.par_iter().filter_map(|&socket| {
            if let Ok(mut stream) =  TcpStream::connect(socket) {
                stream.write(&message).unwrap();
                let mut responce = Vec::<u8>::new(); // using 6 byte buffer
                if let Ok(_) = stream.read_to_end(&mut responce) {
                    return Some(responce)
                }
            }
            let mut dead = dead.lock().expect("can't lock the mutex that stores dead users!");
            dead.insert(socket);
            return None
        }).collect();
        let dead = dead.lock().expect("can't lock the mutex that stores dead users!");
        self.view.retain(|x| !dead.contains(x));

        let mut gm = (self.view.len() as u64).to_le_bytes().to_vec();
        gm.push(4);
        self.gui_sender.send(gm).expect("should be working");

        responces
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
            // if you need to usurp shard 0 because of huge network failure
            if self.usurpingtime.elapsed().as_secs() > USURP_TIME {
                self.usurpingtime = Instant::now();
                self.headshard += 1;
            }


            // updates some gui info
            if self.gui_timer.elapsed().as_secs() > 5 {
                self.gui_timer = Instant::now();

                let mut gm = (self.view.len() as u64).to_le_bytes().to_vec();
                gm.push(4);
                self.gui_sender.send(gm).expect("should be working");
            }







                // while let Async::Ready(Some(fullmsg)) = track_try_unwrap!(self.outer.poll()) {
                //     let msg = fullmsg.message.clone();
                //     let mut m = msg.payload.to_vec();
                //     if let Some(mtype) = m.pop() {
                //         println!("# MESSAGE TYPE: {:?}", mtype);


                //         if mtype == 0 /* transaction someone wants to make */ {
                //             // to avoid spam
                //             let m = m[..std::cmp::min(m.len(),100_000)].to_vec();
                //             if let Ok(t) = bincode::deserialize::<PolynomialTransaction>(&m) {
                //                 if self.save_history {
                //                     let ok = {
                //                         if t.inputs.last() == Some(&1) {
                //                             t.verifystk(&self.stkinfo).is_ok()
                //                         } else {
                //                             let bloom = self.bloom.borrow();
                //                             t.tags.iter().all(|y| !bloom.contains(y.as_bytes())) && t.verify().is_ok()
                //                         }
                //                     };
                //                     if ok {
                //                         self.outer.handle_gossip_now(fullmsg, true);
                //                     }
                //                 } else {
                //                     self.outer.handle_gossip_now(fullmsg, true);
                //                 }
                //             } else {
                //                 self.outer.handle_gossip_now(fullmsg, false);
                //             }
                //         } else if mtype == 3 /* recieved a full block */ {
                //             if let Ok(lastblock) = bincode::deserialize::<NextBlock>(&m) {
                //                 let s = self.readblock(lastblock, m);
                //                 self.outer.handle_gossip_now(fullmsg, s);
                //             } else {
                //                 self.outer.handle_gossip_now(fullmsg, false);
                //             }
                //         } else if mtype == 60 /* < */ { // redo sync request
                //             let mut mynum = self.bnum.to_le_bytes().to_vec();
                //             if self.lightning_yielder { // lightning users don't ask for full blocks
                //                 mynum.push(108); //l
                //             } else {
                //                 mynum.push(102); //f
                //             }
                //             mynum.push(121);
                //             if let Ok(x) = bincode::deserialize(&m) {
                //                 self.outer.dm(mynum, &[x], false);
                //             } else {
                //                 let mut friend = self.outer.plumtree_node().all_push_peers();
                //                 friend.remove(&msg.id.node());
                //                 friend.remove(self.outer.plumtree_node().id());
                //                 let friend = friend.into_iter().collect::<Vec<_>>();
                //                 if let Some(friend) = friend.choose(&mut rand::thread_rng()) {
                //                     println!("asking for help from {:?}",friend);
                //                     self.outer.dm(mynum, &[*friend], false);
                //                 } else {
                //                     println!("you're isolated");
                //                 }
                //             }
                //         } else if mtype == 97 /* a */ {
                //             println!("Someone's trying to connect!!! :3\n(  this means you're recieving connect request >(^.^)>  )");
                //             self.outer.plumtree_node.lazy_push_peers.insert(fullmsg.sender);
                //         } else if mtype == 108 /* l */ { // a lightning block
                //             if self.lightning_yielder {
                //                 if let Ok(lastblock) = bincode::deserialize::<LightningSyncBlock>(&m) {
                //                     self.readlightning(lastblock, m, None); // that whole thing with 3 and 8 makes it super unlikely to get more blocks (expecially for my small thing?)
                //                 }
                //             }
                //         } else if mtype == 113 /* q */ { // they just sent you a ring member
                //             if let Ok(r) = bincode::deserialize::<Vec<Vec<u8>>>(&m) {
                //                 let locs = recieve_ring(&self.rname).unwrap();
                //                 let rmems = r.iter().zip(locs).map(|(x,y)| (y,History::read_raw(x))).collect::<Vec<_>>();
                //                 let mut ringchanged = false;
                //                 for mem in rmems {
                //                     if self.mine.iter().filter(|x| *x.0 == mem.0).count() == 0 {
                //                         self.rmems.insert(mem.0,mem.1);
                //                         ringchanged = true;
                //                     }
                //                 }
                //                 if ringchanged {
                //                     if let Some(outs) = self.outs.clone() {
                //                         let ring = recieve_ring(&self.rname).unwrap();
                //                         let mut got_the_ring = true;
                //                         let mut rlring = ring.iter().map(|x| if let Some(x) = self.rmems.get(x) {x.clone()} else {got_the_ring = false; OTAccount::default()}).collect::<Vec<OTAccount>>();
                //                         rlring.iter_mut().for_each(|x|if let Ok(y)=self.me.receive_ot(&x) {*x = y;});
                //                         let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                //                         if got_the_ring && tx.verify().is_ok() {
                //                             let tx = tx.polyform(&self.rname);
                //                             // tx.verify().unwrap(); // as a user you won't be able to check this
                //                             let mut txbin = bincode::serialize(&tx).unwrap();
                //                             txbin.push(0);
                //                             self.outer.broadcast(txbin);
                //                             println!("transaction ring filled and tx sent!");
                //                         } else {
                //                             println!("you can't make that transaction, user!");
                //                         }
                //                     }
                //                 }
                //             }
                //         } else if mtype == 114 /* r */ { // answer their ring question
                //             if let Ok(r) = recieve_ring(&m) {
                //                 let y = r.iter().map(|y| History::get_raw(y).to_vec()).collect::<Vec<_>>();
                //                 let mut x = bincode::serialize(&y).unwrap();
                //                 x.push(113);
                //                 self.outer.dm(x,&vec![msg.id.node()],false);
                //             }
                //         } else if mtype == 118 /* v */ { // someone announcing they're about to be in the comittee
                //             if let Some(who) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                //                 if let Ok(m) = bincode::deserialize::<NodeId>(&m) {
                //                     if self.queue[self.headshard].contains(&(who as usize)) {
                //                         self.knownnodes.insert(who,m);
                //                         self.outer.handle_gossip_now(fullmsg, true);
                //                     } else {
                //                         self.outer.handle_gossip_now(fullmsg, false);
                //                     }
                //                 } else {
                //                     self.outer.handle_gossip_now(fullmsg, false);
                //                 }
                //             } else {
                //                 self.outer.handle_gossip_now(fullmsg, false);
                //             }
                //         } else if mtype == 121 /* y */ { // someone sent a sync request
                //             let mut i_cant_do_this = true;
                //             if self.save_history {
                //                 if self.sync_returnaddr.is_none() {
                //                     if let Some(theyfast) = m.pop() {
                //                         if let Ok(m) = m.try_into() {
                //                             if theyfast == 108 {
                //                                 self.sync_lightning = true;
                //                                 self.sync_returnaddr = Some(msg.id.node());
                //                                 self.sync_theirnum = u64::from_le_bytes(m);
                //                                 i_cant_do_this = false;
                //                             } else {
                //                                 if !self.lightning_yielder {
                //                                     self.sync_lightning = false;
                //                                     self.sync_returnaddr = Some(msg.id.node());
                //                                     self.sync_theirnum = u64::from_le_bytes(m);
                //                                     i_cant_do_this = false;
                //                                 }
                //                             }
                //                         }
                //                     }
                //                 }
                //             }
                //             if msg.id.node() != *self.outer.plumtree_node().id() && i_cant_do_this {
                //                 let mut friend = self.outer.plumtree_node().all_push_peers().into_iter().collect::<Vec<_>>();
                //                 friend.retain(|&x| x != fullmsg.sender);
                //                 if let Some(friend) = friend.choose(&mut rand::thread_rng()) {
                //                     println!("asking for help from {:?}",friend);
                //                     let mut m = bincode::serialize(friend).unwrap();
                //                     m.push(60);
                //                     self.outer.dm(m, &[*friend], false);
                //                 } else {
                //                     self.outer.dm(vec![60], &[msg.id.node()], false);
                //                     println!("you're isolated");
                //                 }
                //             }
                //         } else /* spam */ {
                //             // self.outer.kill(&fullmsg.sender);
                //             self.outer.handle_gossip_now(fullmsg, false);
                //         }
                //     }
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
            while let Async::Ready(Some(mut m)) = self.gui_reciever.poll().expect("Never fails") {
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
                                    self.rmems = HashMap::new();
                                    for (i,j) in loc.iter().zip(acc) {
                                        println!("i: {}, j.pk: {:?}",i,j.pk.compress());
                                        self.rmems.insert(*i,j);
                                    }
                                    
                                    self.rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + self.ringsize as u16), &self.height);
                                    let ring = recieve_ring(&self.rname).expect("shouldn't fail");
                                    if self.ringsize != 0 {
                                        println!("ring:----------------------------------\n{:?}",ring);
                                        let mut good = false;
                                        for _ in 0..5 {
                                            let mut rnamesend = self.rname.clone();
                                            rnamesend.push(114);
                                            let responces = self.send_message(rnamesend,RING_SEND_TO);
                                            if responces.into_iter().filter(|m| {
                                                if let Ok(r) = bincode::deserialize::<Vec<Vec<u8>>>(m) {
                                                    let locs = recieve_ring(&self.rname).unwrap();
                                                    let rmems = r.iter().zip(locs).map(|(x,y)| (y,History::read_raw(x))).collect::<Vec<_>>();
                                                    let mut ringchanged = false;
                                                    for mem in rmems {
                                                        if self.mine.iter().filter(|x| *x.0 == mem.0).count() == 0 {
                                                            self.rmems.insert(mem.0,mem.1);
                                                            ringchanged = true;
                                                        }
                                                    }
                                                    if ringchanged {
                                                        let ring = recieve_ring(&self.rname).unwrap();
                                                        let mut got_the_ring = true;
                                                        let mut rlring = ring.iter().map(|x| if let Some(x) = self.rmems.get(x) {x.clone()} else {got_the_ring = false; OTAccount::default()}).collect::<Vec<OTAccount>>();
                                                        rlring.iter_mut().for_each(|x|if let Ok(y)=self.me.receive_ot(&x) {*x = y;});
                                                        let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                                                        if got_the_ring && tx.verify().is_ok() {
                                                            let tx = tx.polyform(&self.rname);
                                                            // tx.verify().unwrap(); // as a user you won't be able to check this
                                                            let mut txbin = bincode::serialize(&tx).unwrap();
                                                            txbin.push(0);
                                                            self.send_message(txbin,TRANSACTION_SEND_TO);
                                                            println!("transaction ring filled and tx sent!");
                                                            return true
                                                        } else {
                                                            println!("you can't make that transaction, user!");
                                                        }
                                                    }
                                                }
                                                return false
                                            }).count() != 0 {
                                                good = true;
                                                break
                                            }
                                        }
                                        if !good {
                                            println!("FAILED TO SEND TRANSACTION");
                                        }

                                        txbin = vec![];
                                        
                                    } else {
                                        let mut rlring = ring.iter().map(|&x| self.mine.iter().filter(|(&y,_)| y == x).collect::<Vec<_>>()[0].1.clone()).collect::<Vec<OTAccount>>();
                                        let me = self.me;
                                        rlring.iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x) {*x = y;});
                                        
                                        let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x| (&x.0,&x.1)).collect());

                                        println!("{:?}",rlring.iter().map(|x| x.com.amount).collect::<Vec<_>>());
                                        if tx.verify().is_ok() {
                                            let tx = tx.polyform(&self.rname);
                                            if self.save_history {
                                                tx.verify().unwrap(); // as a user you won't be able to check this
                                            }
                                            txbin = bincode::serialize(&tx).unwrap();
                                            println!("transaction made!");
                                        } else {
                                            txbin = vec![];
                                            println!("you can't make that transaction, user!");
                                        }
                                    }
                                } else {
                                    txbin = vec![];
                                }
                            } else {
                                let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + self.ringsize as u16), &self.height);
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
                        } else {
                            txbin = vec![];
                            println!("somethings wrong with your query!");
                        }
                        // if that tx is valid and ready as far as you know
                        if validtx && !txbin.is_empty() {
                            txbin.push(0);
                            loop {
                                let responces = self.send_message(txbin.clone(), TRANSACTION_SEND_TO);
                                if !responces.is_empty() {
                                    break
                                }
                            }
                            println!("transaction broadcasted");
                        } else {
                            println!("transaction not made right now");
                        }
                    } else if istx == u8::MAX /* panic button */ {
                        
                        let amnt = u64::from_le_bytes(m.drain(..8).collect::<Vec<_>>().try_into().unwrap());
                        // let amnt = Scalar::from(amnt);
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
                                txbin.push(0);
                                self.send_message(txbin.clone(),TRANSACTION_SEND_TO);
                                self.moneyreset = Some(txbin);
                                println!("transaction made!");
                            } else {
                                println!("you can't make that transaction, user!");
                            }
                        }



                        self.mine = HashMap::new();
                        self.me = newacc;
                        self.key = self.me.stake_acc().receive_ot(&self.me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap().sk.unwrap();
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
                        let mut gm = (self.view.len() as u64).to_le_bytes().to_vec();
                        gm.push(4);
                        self.gui_sender.send(gm).expect("should be working");
                        loop {
                            let mut mynum = self.bnum.to_le_bytes().to_vec();
                            mynum.push(121);
                            let responces = self.send_message(mynum, SYNC_SEND_TO);
                            if responces.len() == 0 {
                                break
                            }
                        }

                        // asdfasdffds

                    } else if istx == 42 /* * */ { // entry address
                        let socket = format!("{}:{}",String::from_utf8_lossy(&m),OUTSIDER_PORT);
                        println!("{}",socket);
                        match TcpStream::connect(socket) {
                            Ok(mut stream) => {
                                let msg = vec![101u8];
                                stream.write(&msg).unwrap();
                                println!("Asking for entrance, awaiting reply...");
                    
                                let mut data = Vec::<u8>::new(); // using 6 byte buffer
                                match stream.read_to_end(&mut data) {
                                    Ok(_) => {
                                        if let Ok(x) = bincode::deserialize(&data) {
                                            self.view = x;
                                        } else {
                                            println!("They didn't send a view!")
                                        }
                                    },
                                    Err(e) => {
                                        println!("Failed to receive data: {}", e);
                                    }
                                }
                            },
                            Err(e) => {
                                println!("Failed to connect: {}", e);
                            }
                        }

                    } else if istx == 64 /* @ */ {
                        let mut gm = (self.view.len() as u64).to_le_bytes().to_vec();
                        gm.push(4);
                        self.gui_sender.send(gm).expect("should be working");
                    } else if istx == 0 {
                        println!("Saving!");
                        self.save();
                        self.gui_sender.send(vec![253]).unwrap();
                    }
                }
            }












        }
        Ok(Async::NotReady)
    }
}