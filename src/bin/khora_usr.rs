#[macro_use]
// #[allow(unreachable_code)]
extern crate trackable;

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll};
// use khora::seal::BETA;
use rand::prelude::SliceRandom;
use std::{cmp, thread};
use std::fs::File;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use trackable::error::MainError;
use crossbeam::channel;


use khora::{account::*, gui};
use curve25519_dalek::scalar::Scalar;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::time::{Duration, Instant};
use khora::transaction::*;
use curve25519_dalek::ristretto::{CompressedRistretto};
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use khora::validation::*;
use khora::ringmaker::*;
use serde::{Serialize, Deserialize};
use khora::validation::{
    NUMBER_OF_VALIDATORS, QUEUE_LENGTH, PERSON0,
    ACCOUNT_COMBINE, READ_TIMEOUT, WRITE_TIMEOUT, NONCEYNESS,
    reward, blocktime,
};
use colored::Colorize;





/// the outsider port
const OUTSIDER_PORT: u16 = 8335;
/// the number of nodes to send each message to
const TRANSACTION_SEND_TO: usize = 1;

fn main() -> Result<(), MainError> {    
    // this is the number of shards they keep track of
    let max_shards = 64usize; /* this if for testing purposes... there IS NO MAX SHARDS */
    



    // println!("computer socket: {}\nglobal socket: {}",local_socket,global_socket);
    let executor = track_any_err!(ThreadPoolExecutor::new())?;

    let (ui_sender, urecv) = channel::unbounded::<Vec<u8>>();
    let (usend, ui_reciever) = channel::unbounded();



    
    // the myUsr file only exists if you already have an account made
    let setup = !Path::new("myUsr").exists();
    if setup {
        // everyone agrees this person starts with 1 khora token
        let initial_history = (PERSON0,1u64);

        thread::spawn(move || {
            let pswrd = urecv.recv().unwrap();
            
            let will_stk = urecv.recv().unwrap()[0] == 0;
            println!("password:\n{:?}",pswrd);

            let me = Account::new(&pswrd);
            if will_stk {
                History::initialize();
            }
    
    
            let mut allnetwork = HashMap::new();
            allnetwork.insert(initial_history.0, (0u64,None));
            // creates the node with the specified conditions then saves it to be used from now on
            let node = KhoraNode {
                sendview: vec![],
                allnetwork,
                save_history: will_stk,
                lasttags: vec![],
                me,
                mine: HashMap::new(),
                reversemine: HashMap::new(),
                nmine: None,
                nheight: 0,
                stkinfo: vec![initial_history.clone()],
                queue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|_| 0).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                exitqueue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                comittee: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|_| 0).collect::<Vec<usize>>()).collect::<Vec<_>>(),
                lastname: Scalar::one().as_bytes().to_vec(),
                bnum: 0u64,
                lastbnum: 0u64,
                height: 0u64,
                alltagsever: HashSet::new(),
                headshard: 0,
                rmems: HashMap::new(),
                rname: vec![],
                gui_sender: usend.clone(),
                gui_reciever: channel::unbounded().1,
                moneyreset: None,
                cumtime: 0f64,
                blocktime: blocktime(0.0),
                ringsize: 5,
                gui_timer: Instant::now(),
            };
            node.save();

            let mut info = bincode::serialize(&
                vec![
                    bincode::serialize(&node.me.name()).expect("should work"),
                    bincode::serialize(&node.me.nonanony_acc().name()).expect("should work"),
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
        let native_options = eframe::NativeOptions::default();
        eframe::run_native(Box::new(app), native_options);
    } else {
        let mut node = KhoraNode::load(usend, urecv);
        let mut mymoney = node.mine.iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
        mymoney.extend(node.nmine.iter().map(|x| x[1]).sum::<u64>().to_le_bytes());
        mymoney.push(0);
        node.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui

        let app = gui::user::KhoraUserGUI::new(
            ui_reciever,
            ui_sender,
            node.me.name(),
            node.me.nonanony_acc().name(),
            node.me.sk.as_bytes().to_vec(),
            node.me.vsk.as_bytes().to_vec(),
            node.me.ask.as_bytes().to_vec(),
            false,
        );
        let native_options = eframe::NativeOptions::default();
        thread::spawn(move || {
            node.attempt_sync();
            executor.spawn(node);
            track_any_err!(executor.run()).unwrap();
        });
        eframe::run_native(Box::new(app), native_options);
    }
    







}

#[derive(Clone, Serialize, Deserialize, Debug)]
/// the information that you save to a file when the app is off (not including gui information like saved friends)
struct SavedNode {
    sendview: Vec<SocketAddr>,
    save_history: bool, //just testing. in real code this is true; but i need to pretend to be different people on the same computer
    me: Account,
    mine: HashMap<u64, OTAccount>,
    nmine: Option<[u64; 2]>, // [location, amount]
    nheight: u64,
    reversemine: HashMap<CompressedRistretto, u64>,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bnum: u64,
    lastbnum: u64,
    height: u64,
    alltagsever: HashSet<CompressedRistretto>,
    headshard: usize,
    rname: Vec<u8>,
    moneyreset: Option<(Vec<u8>,CompressedRistretto)>,
    cumtime: f64,
    blocktime: f64,
    ringsize: u8,
    lasttags: Vec<PolynomialTransaction>,
}

/// the node used to run all the networking
struct KhoraNode {
    gui_sender: channel::Sender<Vec<u8>>,
    gui_reciever: channel::Receiver<Vec<u8>>,
    sendview: Vec<SocketAddr>,
    allnetwork: HashMap<CompressedRistretto,(u64,Option<SocketAddr>)>,
    save_history: bool, // do i really want this? yes?
    me: Account,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bnum: u64,
    lastbnum: u64,
    nmine: Option<[u64; 2]>, // [location, amount]
    nheight: u64,
    height: u64,
    mine: HashMap<u64, OTAccount>,
    reversemine: HashMap<CompressedRistretto, u64>,
    alltagsever: HashSet<CompressedRistretto>,
    headshard: usize,
    rmems: HashMap<u64,OTAccount>,
    rname: Vec<u8>,
    moneyreset: Option<(Vec<u8>,CompressedRistretto)>,
    cumtime: f64,
    blocktime: f64,
    gui_timer: Instant,
    ringsize: u8,
    lasttags: Vec<PolynomialTransaction>,
}

impl KhoraNode {
    /// saves the important information like staker state and block number to a file: "myUsr"
    fn save(&self) {
        if self.moneyreset.is_none() {
            let sn = SavedNode {
                lasttags: self.lasttags.clone(),
                sendview: self.sendview.clone(),
                save_history: self.save_history,
                me: self.me,
                mine: self.mine.clone(),
                nheight: self.nheight,
                nmine: self.nmine.clone(), // [location, amount]
                reversemine: self.reversemine.clone(),
                stkinfo: self.stkinfo.clone(),
                queue: self.queue.clone(),
                exitqueue: self.exitqueue.clone(),
                comittee: self.comittee.clone(),
                lastname: self.lastname.clone(),
                bnum: self.bnum,
                lastbnum: self.lastbnum,
                height: self.height,
                alltagsever: self.alltagsever.clone(),
                headshard: self.headshard.clone(),
                rname: self.rname.clone(),
                moneyreset: self.moneyreset.clone(),
                cumtime: self.cumtime,
                blocktime: self.blocktime,
                ringsize: self.ringsize,
            }; // just redo initial conditions on the rest
            let mut sn = bincode::serialize(&sn).unwrap();
            let mut f = File::create("myUsr").unwrap();
            f.write_all(&mut sn).unwrap();
        }
    }

    /// loads the node information from a file: "myUsr"
    fn load(gui_sender: channel::Sender<Vec<u8>>, gui_reciever: channel::Receiver<Vec<u8>>) -> KhoraNode {
        let mut buf = Vec::<u8>::new();
        let mut f = File::open("myUsr").unwrap();
        f.read_to_end(&mut buf).unwrap();

        let sn = bincode::deserialize::<SavedNode>(&buf).unwrap();

        let allnetwork = sn.stkinfo.iter().enumerate().map(|(i,x)| (x.0,(i as u64,None))).collect::<HashMap<_,_>>();
        KhoraNode {
            gui_sender,
            gui_reciever,
            sendview: sn.sendview.clone(),
            save_history: sn.save_history,
            allnetwork,
            me: sn.me,
            mine: sn.mine.clone(),
            reversemine: sn.reversemine.clone(),
            stkinfo: sn.stkinfo.clone(),
            queue: sn.queue.clone(),
            exitqueue: sn.exitqueue.clone(),
            nheight: sn.nheight,
            nmine: sn.nmine.clone(), // [location, amount]
            comittee: sn.comittee.clone(),
            lastname: sn.lastname.clone(),
            bnum: sn.bnum,
            lastbnum: sn.lastbnum,
            height: sn.height,
            alltagsever:sn.alltagsever.clone(),
            headshard: sn.headshard.clone(),
            rmems: HashMap::new(),
            rname: vec![],
            moneyreset: sn.moneyreset,
            cumtime: sn.cumtime,
            blocktime: sn.blocktime,
            gui_timer: Instant::now(),
            ringsize: sn.ringsize,
            lasttags: sn.lasttags.clone(),
        }
    }

    /// reads a lightning block and saves information when appropriate and returns if you accepted the block
    fn readlightning(&mut self, lastlightning: LightningSyncBlock, m: Vec<u8>, save: bool) {
        if lastlightning.bnum >= self.bnum {
            let com = self.comittee.par_iter().map(|x| x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>();
            if lastlightning.shards.len() == 0 {
                // println!("Error in block verification: there is no shard");
                return ();
            }
            let v = if (lastlightning.shards[0] as usize >= self.headshard) && (lastlightning.last_name == self.lastname) {
                lastlightning.verify_multithread(&com[lastlightning.shards[0] as usize], &self.stkinfo).is_ok()
            } else {
                false
            };
            if v  {
                
                // let t = Instant::now();
                if save {
                    // saves your current information BEFORE reading the new block. It's possible a leader is trying to cause a fork which can only be determined 1 block later based on what the comittee thinks is real
                    self.save();
                }
                // println!("{}",format!("{}",t.elapsed().as_millis()).red());
                self.headshard = lastlightning.shards[0] as usize;

                // println!("=========================================================\nyay!");


                // if you're synicng, you just infer the empty blocks that no one saves
                for _ in self.bnum..lastlightning.bnum {
                    // println!("I missed a block!");
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
                    // let t = Instant::now();
                    // println!("{}",format!("scan stake: {}",t.elapsed().as_millis()).yellow());
                    // let t = Instant::now();
                    let mut guitruster = lastlightning.scan(&self.me, &mut self.mine, &mut self.reversemine, &mut self.height, &mut self.alltagsever);
                    // println!("{}",format!("scan: {}",t.elapsed().as_millis()).yellow());
                    guitruster = lastlightning.scannonanony(&self.me, &mut self.nmine, &mut self.nheight) || guitruster;

                    if save {
                        self.gui_sender.send(vec![!guitruster as u8,1]);
                    }
                    
                    // let t = Instant::now();
                    lastlightning.scan_as_noone(&mut self.stkinfo, &mut vec![], false, &mut self.allnetwork, &mut self.queue, &mut self.exitqueue, &mut self.comittee, reward, self.save_history);
                    // println!("{}",format!("no one: {}",t.elapsed().as_millis()).yellow());

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
                


                // let t = Instant::now();
                // send info to the gui
                // println!("block {} name: {:?}",self.bnum, self.lastname);

                
                self.lasttags.clone().into_iter().for_each(|x| {
                    if lastlightning.info.tags.contains(&x.tags[0]) {
                        self.lasttags = vec![];
                    }    
                });

                self.cumtime += self.blocktime;
                self.blocktime = blocktime(self.cumtime);

                if self.send_panic_or_stop(&lastlightning.info.tags) {
                    if save {
                        if let Some((x,_)) = self.moneyreset.clone() {
                            self.send_message(x, TRANSACTION_SEND_TO);
                        }
                    }
                }
                // println!("block reading process done!!!");

                // println!("{}",format!("{}",t.elapsed().as_millis()).bright_yellow());


            }
        }
    }

    /// runs the operations needed for the panic button to work
    fn send_panic_or_stop(&mut self, tags: &Vec<CompressedRistretto>) -> bool {
        if let Some(m) = &self.moneyreset {
            if tags.contains(&m.1) {
                self.moneyreset = None;
                false
            } else {
                true
            }
        } else {
            false
        }
    }

    /// returns the responces of each person you sent it to and deletes those who are dead from the view
    fn send_message(&mut self, message: Vec<u8>, recipients: usize) -> Vec<Vec<u8>> {

        let mut rng = &mut rand::thread_rng();
        self.sendview.shuffle(&mut rng);
        let mut dead = HashSet::<SocketAddr>::new();
        let responces = self.sendview[..cmp::min(recipients,self.sendview.len())].iter().filter_map(|socket| {
            if let Ok(mut stream) =  TcpStream::connect(socket) {
                stream.set_read_timeout(READ_TIMEOUT);
                stream.set_write_timeout(WRITE_TIMEOUT);
                println!("connected...");
                if stream.write_all(&message).is_ok() {
                    println!("request made...");
                    let mut responce = Vec::<u8>::new(); // using 6 byte buffer
                    if let Ok(_) = stream.read_to_end(&mut responce) {
                        println!("{}","RESPONCE HERD".green());
                        return Some(responce)
                    }
                }
            }
            dead.insert(*socket);
            return None
        }).collect();
        self.sendview.retain(|x| !dead.contains(x));

        let mut gm = (self.sendview.len() as u64).to_le_bytes().to_vec();
        gm.push(4);
        self.gui_sender.send(gm).expect("should be working");


        println!("{}","FINISHED LISTENING".red());
        responces
    }

    /// returns the responces of each person you sent it to and deletes those who are dead from the view
    fn fill_ring(&mut self) {

        let mut rname = self.rname.clone();
        rname.push(114);

        let ring = recieve_ring(&self.rname).expect("shouldn't fail");



        if self.ringsize > 0 {
            if self.save_history {
                for member in ring {
                    if let Some(x) = self.rmems.get(&member) {
                        if x.tag.is_some() {
                            self.rmems.insert(member,OTAccount::summon_ota(&History::get(&member)));
                        }
                    }
                }
            } else {
                let mut rng = &mut rand::thread_rng();
                self.sendview.shuffle(&mut rng);
                let mut memloc = 0usize;
                if let Ok(mut stream) =  TcpStream::connect(&self.sendview[..]) {
                    stream.set_read_timeout(READ_TIMEOUT);
                    stream.set_write_timeout(WRITE_TIMEOUT);
                    println!("connected...");
                    if stream.write_all(&rname).is_ok() {
                        println!("request made...");
                        let mut member = [0u8;64]; // using 6 byte buffer
                        while stream.read_exact(&mut member).is_ok() {
                                if let Some(x) = self.rmems.get(&ring[memloc]) {
                                    if x.tag.is_none() {
                                        print!("{}","!".red());
                                        self.rmems.insert(ring[memloc],History::read_raw(&member));
                                    }
                                } else {
                                    self.rmems.insert(ring[memloc],History::read_raw(&member));
                                }
                                memloc += 1;
                        }
                    }
                }
            }
        } else {
            for (i,j) in self.mine.iter() {
                self.rmems.insert(*i,j.clone());
            }

        }
    }

    /// returns the responces of each person you sent it to and deletes those who are dead from the view
    fn attempt_sync(&mut self) {

        let mut rng = &mut rand::thread_rng();
        self.sendview.shuffle(&mut rng);
        println!("attempting sync...");
        while let Ok(mut stream) =  TcpStream::connect(&self.sendview[..]) {
            stream.set_read_timeout(READ_TIMEOUT);
            stream.set_write_timeout(WRITE_TIMEOUT);
            println!("connected...");
            let mut m = self.bnum.to_le_bytes().to_vec();
            m.push(121);
            if stream.write_all(&m).is_ok() {
                println!("request made...");
                let mut ok = [0;8];
                if stream.read_exact(&mut ok).is_ok() {
                    let syncnum = u64::from_le_bytes(ok);
                    self.gui_sender.send(ok.iter().chain(&[7u8]).cloned().collect()).unwrap();
                    println!("responce valid. syncing now...");
                    let mut blocksize = [0u8;8];
                    let mut counter = 0;
                    while stream.read_exact(&mut blocksize).is_ok() {
                        // println!(".");
                        counter += 1;
                        // let tt = Instant::now();
                        let bsize = u64::from_le_bytes(blocksize) as usize;
                        println!("block: {} of {} -- {} bytes",self.bnum,syncnum,bsize);
                        let mut serialized_block = vec![0u8;bsize];
                        if stream.read_exact(&mut serialized_block).is_err() {
                            println!("couldn't read the bytes");
                        }
                        if let Ok(lastblock) = bincode::deserialize::<LightningSyncBlock>(&serialized_block) {
                            
                            // let t = Instant::now();
                            self.readlightning(lastblock, serialized_block, (counter%1000 == 0) || (self.bnum >= syncnum-1));
                            // println!("{}",format!("block reading time: {}ms",t.elapsed().as_millis()).bright_yellow().bold());
                        } else {
                            println!("they send a fake block");
                        }

                        let mut thisbnum = self.bnum.to_le_bytes().to_vec();
                        thisbnum.push(2);
                        self.gui_sender.send(thisbnum).expect("something's wrong with the communication to the gui");

                        let mut mymoney = self.mine.iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
                        mymoney.extend(self.nmine.iter().map(|x| x[1]).sum::<u64>().to_le_bytes());
                        mymoney.push(0);
                        self.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui");

                        // println!("{}",format!("total time: {}ms",tt.elapsed().as_millis()).green().bold());
                        // println!(".");
                    }
                    break
                }
            }
        }
        if self.lasttags.is_empty() {
            self.gui_sender.send(vec![5]).expect("something's wrong with the communication to the gui");
        }
        self.gui_sender.send([0u8;8].iter().chain(&[7u8]).cloned().collect()).unwrap();
        if self.mine.len() >= ACCOUNT_COMBINE {
            self.gui_sender.send(vec![8]);
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


            // updates some gui info
            if self.gui_timer.elapsed().as_secs() > 5 {
                self.gui_timer = Instant::now();

                let mut gm = (self.sendview.len() as u64).to_le_bytes().to_vec();
                gm.push(4);
                self.gui_sender.send(gm).expect("should be working");
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
            // interacting with the gui
            while let Ok(mut m) = self.gui_reciever.try_recv() {
                println!("got message from gui!\n{}",String::from_utf8_lossy(&m));
                if let Some(istx) = m.pop() {
                    if istx == 33 /* ! */ { // a transaction
                        let mut mymoney = self.mine.iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
                        mymoney.extend(self.nmine.iter().map(|x| x[1]).sum::<u64>().to_le_bytes());
                        mymoney.push(0);
                        self.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui");

                        let txtype = m.pop().unwrap();

                        if txtype == 33 /* ! */ {
                            self.ringsize = m.pop().unwrap();
                        }

                        let mut outs = vec![];
                        let mut validtx = true;
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
                                            println!("address types incorrectly");
                                            pks.push(RISTRETTO_BASEPOINT_POINT);
                                            validtx = false;
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
                                    println!("amounts {:?}",x);
                                    // let y = x/2u64.pow(BETA as u32) + 1;
                                    // println!("need to split this up into {} txses!",y);
                                    let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                                    // for _ in 0..y {
                                    //     let amnt = Scalar::from(x/y);
                                    //     outs.push((recv,amnt));
                                    // }
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
                        if txtype == 33 /* ! */ {

                            // if you need help with ring generation
                            if self.mine.len() > 0 && validtx && outs.len() > 1 {
                                let (loc, acc): (Vec<u64>,Vec<OTAccount>) = self.mine.iter().map(|x|(*x.0,x.1.clone())).unzip();
            

                                let m = self.mine.iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>() - outs.iter().map(|x| x.1).sum::<Scalar>();
                                let x = outs.len() - 1;
                                outs[x].1 = m;


                                println!("loc: {:?}",loc);
                                println!("height: {}",self.height);
                                self.rmems = HashMap::new();
                                for (i,j) in loc.iter().zip(acc) {
                                    println!("i: {}, j.pk: {:?}",i,j.pk.compress());
                                    self.rmems.insert(*i,j);
                                }
                                
                                self.rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + self.ringsize as u16), &self.height);
                                let ring = recieve_ring(&self.rname).expect("shouldn't fail");


                                self.fill_ring();
                                println!("ring:----------------------------------\n{:?}",ring);
                                let mut rnamesend = self.rname.clone();
                                rnamesend.push(114);
                                let mut got_the_ring = true;
                                let rlring = ring.iter().map(|x| if let Some(x) = self.rmems.get(x) {x.clone()} else {got_the_ring = false; OTAccount::default()}).collect::<Vec<OTAccount>>();
                                let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                                if got_the_ring && tx.verify().is_ok() {
                                    let tx = tx.polyform(&self.rname);
                                    let mut txbin = bincode::serialize(&tx).unwrap();
                                    txbin.push(0);
                                    self.lasttags.push(tx);
                                    self.send_message(txbin,TRANSACTION_SEND_TO);
                                    println!("{}","==========================\nTRANDACTION SENT\n==========================".bright_yellow().bold());
                                } else {
                                    println!("{}","YOU DIDNT GET THE RING".red().bold());
                                }


                            } else {
                                println!("{}","TRANSACTION INVALID".red().bold());
                            }
                        } else if txtype ==  64 /* ?+1 */ {
                            let m = Scalar::from(self.nmine.unwrap()[1]) - outs.iter().map(|x| x.1).sum::<Scalar>();
                            let x = outs.len() - 1;
                            outs[x].1 = m;

                            
                            let (loc, amnt): (Vec<u64>,Vec<u64>) = self.nmine.iter().map(|x|(x[0] as u64,x[1].clone())).unzip();
                            let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                            let tx = Transaction::spend_ring_nonce(&inps, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>(),self.bnum/NONCEYNESS);
                            if tx.verify_nonce(self.bnum/NONCEYNESS).is_ok() {
                                let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                                loc.push(2);
                                let tx = tx.polyform(&loc);

                                let mut txbin = bincode::serialize(&tx).unwrap();
                                txbin.push(0);
                                self.lasttags.push(tx);
                                self.send_message(txbin,TRANSACTION_SEND_TO);
                                println!("{}","==========================\nTRANDACTION SENT\n==========================".bright_yellow().bold());
                            } else {
                                println!("{}","TRANSACTION INVALID".red().bold());
                            }
                        }
                    } else if istx == 2 /* divide accounts so you can make faster tx */ {


                        let fee = Scalar::from(u64::from_le_bytes(m.try_into().unwrap()));


                        for mine in self.mine.clone().iter().collect::<Vec<_>>().chunks(ACCOUNT_COMBINE) {
                            if mine.len() > 1 {
                                let (loc, acc): (Vec<u64>,Vec<OTAccount>) = mine.iter().map(|x|(*x.0,x.1.clone())).unzip();
    
                                let mymoney = acc.iter().map(|x| x.com.amount.unwrap()).sum::<Scalar>();
                                let outs = vec![(self.me,mymoney-fee)];
    
                                self.rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + self.ringsize as u16), &self.height);
        
        
                                self.rmems = HashMap::new();
                                for (i,j) in loc.iter().zip(acc) {
                                    println!("i: {}, j.pk: {:?}",i,j.pk.compress());
                                    self.rmems.insert(*i,j);
                                }
                                
                                self.fill_ring();
    
                                let ring = recieve_ring(&self.rname);
                                println!("ring:----------------------------------\n{:?}",ring);
                                let mut rnamesend = self.rname.clone();
                                rnamesend.push(114);
                                let mut got_the_ring = true;
                                let rlring = ring.expect("shouldn't fail").iter().map(|x| if let Some(x) = self.rmems.get(x) {x.clone()} else {got_the_ring = false; OTAccount::default()}).collect::<Vec<OTAccount>>();
                                let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                                if got_the_ring && tx.verify().is_ok() {
                                    let tx = tx.polyform(&self.rname);
                                    let mut txbin = bincode::serialize(&tx).unwrap();
                                    txbin.push(0);
                                    self.lasttags.push(tx);
                                    self.send_message(txbin,TRANSACTION_SEND_TO);
                                    println!("{}","==========================\nTRANDACTION SENT\n==========================".bright_yellow().bold());
                                } else {
                                    println!("{}",if !got_the_ring {"YOU DIDNT GET THE RING".red().bold()} else {"TRANSACTION INVALID".red().bold()});
                                }
                            }
                        }
                        println!("{}","done with that".green());


                    } else if istx == u8::MAX /* panic button */ {
                        
                        let amnt = u64::from_le_bytes(m.drain(..8).collect::<Vec<_>>().try_into().unwrap());
                        let amnt = self.mine.iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>() - Scalar::from(amnt);
                        let amnt = u64::from_le_bytes(amnt.as_bytes()[..8].to_vec().try_into().unwrap());


                        let newacc = Account::new(&format!("{}",String::from_utf8_lossy(&m)));

                        if self.mine.len() > 0 {
                            let loc = self.mine.iter().map(|(&x,_)|x).collect::<Vec<_>>();

                            println!("remembered owned accounts");
                            let rname = generate_ring(&loc.iter().map(|&x|x as usize).collect::<Vec<_>>(), &(loc.len() as u16), &self.height);
                            let ring = recieve_ring(&rname).expect("shouldn't fail");

                            println!("made rings");
                            /* you don't use a ring for panics (the ring is just your own accounts) */ 
                            let rlring = ring.iter().map(|&x| self.mine.iter().filter(|(&y,_)| y == x).collect::<Vec<_>>()[0].1.clone()).collect::<Vec<OTAccount>>();
                            
                            

                            let mut outs = vec![];
                            // let y = amnt/2u64.pow(BETA as u32);
                            // let mut tot = 0u64;
                            // for _ in 0..y {
                            //     tot += amnt/y;
                            //     let amnt = Scalar::from(amnt/y);
                            //     outs.push((&newacc,amnt));
                            // }
                            // let amnt = Scalar::from(tot - amnt); // prob that this is less than 0 is crazy small for reasonable fee
                            // outs.push((&newacc,amnt));
                            outs.push((&newacc,Scalar::from(amnt)));

                            let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x| (x.0,&x.1)).collect());

                            println!("{:?}",rlring.iter().map(|x| x.com.amount).collect::<Vec<_>>());
                            println!("{:?}",amnt);
                            if tx.verify().is_ok() {
                                let tx = tx.polyform(&rname);
                                if self.save_history {
                                    tx.verify().unwrap();
                                }
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                txbin.push(0);
                                
                                let responces = self.send_message(txbin.clone(), TRANSACTION_SEND_TO);
                                if !responces.is_empty() {
                                    println!("responce 0 is: {:?}",responces);
                                }
                                self.moneyreset = Some((txbin,tx.tags[0]));
                                println!("transaction made!");
                            } else {
                                println!("you can't make that transaction, user!");
                            }
                        }



                        self.mine = HashMap::new();
                        self.reversemine = HashMap::new();
                        self.alltagsever = HashSet::new();
                        self.me = newacc;

                        let mut info = bincode::serialize(&
                            vec![
                            bincode::serialize(&self.me.name()).expect("should work"),
                            bincode::serialize(&self.me.nonanony_acc().name()).expect("should work"),
                            bincode::serialize(&self.me.sk.as_bytes().to_vec()).expect("should work"),
                            bincode::serialize(&self.me.vsk.as_bytes().to_vec()).expect("should work"),
                            bincode::serialize(&self.me.ask.as_bytes().to_vec()).expect("should work"),
                            ]
                        ).expect("should work");
                        info.push(254);
                        self.gui_sender.send(info).expect("should work");

                    } else if istx == 121 /* y */ { // you clicked sync
                        let mut gm = (self.sendview.len() as u64).to_le_bytes().to_vec();
                        gm.push(4);
                        self.gui_sender.send(gm).expect("should be working");
                        self.attempt_sync();
                        self.gui_sender.send(vec![self.blocktime as u8,128]).expect("something's wrong with the communication to the gui");


                    } else if istx == 42 /* * */ { // entry address
                        let socket = format!("{}:{}",String::from_utf8_lossy(&m),OUTSIDER_PORT);
                        println!("{}",socket);
                        match TcpStream::connect(socket) {
                            Ok(mut stream) => {
                                let msg = vec![101u8];
                                if stream.write_all(&msg).is_err() {
                                    println!("failed to write entrypoint message");
                                }
                                println!("Asking for entrance, awaiting reply...");
                    
                                let mut data = Vec::<u8>::new(); // using 6 byte buffer
                                match stream.read_to_end(&mut data) {
                                    Ok(_) => {
                                        if let Ok(x) = bincode::deserialize(&data) {
                                            self.sendview = x;
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
                        let mut gm = (self.sendview.len() as u64).to_le_bytes().to_vec();
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
