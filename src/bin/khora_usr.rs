#[macro_use]
// #[allow(unreachable_code)]
extern crate trackable;

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll};
use khora::network::{write_timeout, read_to_end_timeout, read_timeout};
use khora::vechashmap::follow;
// use khora::seal::BETA;
use rand::prelude::SliceRandom;
use std::borrow::Borrow;
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
use std::time::Instant;
use khora::transaction::*;
use curve25519_dalek::ristretto::{CompressedRistretto};
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use khora::validation::*;
use khora::ringmaker::*;
use serde::{Serialize, Deserialize};
use khora::validation::{
    NUMBER_OF_VALIDATORS, QUEUE_LENGTH, PERSON0, LONG_TERM_SHARDS,
    ACCOUNT_COMBINE, READ_TIMEOUT, WRITE_TIMEOUT, NONCEYNESS,
    OUTSIDER_PORT, TRANSACTION_SEND_TO,
    reward, blocktime, set_comittee_n_user, comittee_n_user,
};
use colored::Colorize;






fn main() -> Result<(), MainError> {



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
            
            println!("password:\n{:?}",pswrd);

            let me = Account::new(&pswrd);
    
    
            // creates the node with the specified conditions then saves it to be used from now on
            let node = KhoraNode {
                sendview: vec![],
                lasttags: vec![],
                me,
                mine: HashMap::new(),
                reversemine: HashMap::new(),
                nmine: None,
                nheight: 0,
                stkinfo: vec![initial_history.clone()],
                queue: (0..LONG_TERM_SHARDS).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|_| 0).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                exitqueue: (0..LONG_TERM_SHARDS).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
                comittee: (0..LONG_TERM_SHARDS).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|_| 0).collect::<Vec<usize>>()).collect::<Vec<_>>(),
                lastname: Scalar::one().as_bytes().to_vec(),
                bnum: 0u64,
                lastbnum: 0u64,
                height: 0u64,
                alltagsever: HashSet::new(),
                rmems: HashMap::new(),
                rname: vec![],
                gui_sender: usend.clone(),
                gui_reciever: channel::unbounded().1,
                cumtime: 0f64,
                blocktime: blocktime(0.0),
                ringsize: 5,
                gui_timer: Instant::now(),
                lastnonanony: None,
                paniced: None,
            };
            node.save();

            let mut info = bincode::serialize(&
                vec![
                    bincode::serialize(&node.me.name()).unwrap(),
                    bincode::serialize(&node.me.nonanony_acc().name()).unwrap(),
                    bincode::serialize(&node.me.sk.as_bytes().to_vec()).unwrap(),
                    bincode::serialize(&node.me.vsk.as_bytes().to_vec()).unwrap(),
                    bincode::serialize(&node.me.ask.as_bytes().to_vec()).unwrap(),
                ]
            ).unwrap();
            info.push(254);
            usend.send(info).unwrap();


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
        mymoney.extend(node.nmine.iter().map(|x| x.1).sum::<u64>().to_le_bytes());
        mymoney.push(0);
        node.gui_sender.send(mymoney).unwrap(); // this is how you send info to the gui

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
    me: Account,
    mine: HashMap<u64, OTAccount>,
    nmine: Option<(usize,u64)>, // [location, amount]
    nheight: usize,
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
    rname: Vec<u8>,
    cumtime: f64,
    blocktime: f64,
    ringsize: u8,
    lasttags: Vec<CompressedRistretto>,
    lastnonanony: Option<usize>,
    paniced: Option<(Account,Option<PolynomialTransaction>,Option<(usize,u64)>,u64)>
}

/// the node used to run all the networking
struct KhoraNode {
    gui_sender: channel::Sender<Vec<u8>>,
    gui_reciever: channel::Receiver<Vec<u8>>,
    sendview: Vec<SocketAddr>,
    me: Account,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bnum: u64,
    lastbnum: u64,
    nmine: Option<(usize,u64)>, // [location, amount]
    nheight: usize,
    height: u64,
    mine: HashMap<u64, OTAccount>,
    reversemine: HashMap<CompressedRistretto, u64>,
    alltagsever: HashSet<CompressedRistretto>,
    rmems: HashMap<u64,OTAccount>,
    rname: Vec<u8>,
    cumtime: f64,
    blocktime: f64,
    gui_timer: Instant,
    ringsize: u8,
    lasttags: Vec<CompressedRistretto>,
    lastnonanony: Option<usize>,
    paniced: Option<(Account,Option<PolynomialTransaction>,Option<(usize,u64)>,u64)>
}

impl KhoraNode {
    /// saves the important information like staker state and block number to a file: "myUsr"
    fn save(&self) {
        let sn = SavedNode {
            lasttags: self.lasttags.clone(),
            sendview: self.sendview.clone(),
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
            rname: self.rname.clone(),
            paniced: self.paniced.clone(),
            cumtime: self.cumtime,
            blocktime: self.blocktime,
            ringsize: self.ringsize,
            lastnonanony: self.lastnonanony,
        }; // just redo initial conditions on the rest
        let mut sn = bincode::serialize(&sn).unwrap();
        let mut f = File::create("myUsr").unwrap();
        f.write_all(&mut sn).unwrap();
    }

    /// loads the node information from a file: "myUsr"
    fn load(gui_sender: channel::Sender<Vec<u8>>, gui_reciever: channel::Receiver<Vec<u8>>) -> KhoraNode {
        let mut buf = Vec::<u8>::new();
        let mut f = File::open("myUsr").unwrap();
        f.read_to_end(&mut buf).unwrap();

        let sn = bincode::deserialize::<SavedNode>(&buf).unwrap();

        KhoraNode {
            gui_sender,
            gui_reciever,
            sendview: sn.sendview.clone(),
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
            rmems: HashMap::new(),
            rname: vec![],
            paniced: sn.paniced,
            cumtime: sn.cumtime,
            blocktime: sn.blocktime,
            gui_timer: Instant::now(),
            ringsize: sn.ringsize,
            lasttags: sn.lasttags.clone(),
            lastnonanony: sn.lastnonanony,
        }
    }

    /// reads a lightning block and saves information when appropriate and returns if you accepted the block
    fn readlightning(&mut self, lastlightning: LightningSyncBlock, m: Vec<u8>, save: bool) {
        if lastlightning.bnum >= self.bnum {
            let v = if lastlightning.last_name == self.lastname {
                lastlightning.verify_multithread_user(&comittee_n_user(lastlightning.shard as usize, &self.comittee, &self.stkinfo), &self.stkinfo).is_ok()
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

                // println!("=========================================================\nyay!");
                self.send_panic_or_stop(&lastlightning,self.nheight.clone(), save);


                // if you're synicng, you just infer the empty blocks that no one saves
                for _ in self.bnum..lastlightning.bnum {
                    // println!("I missed a block!");
                    let reward = reward(self.cumtime,self.blocktime);
                    self.cumtime += self.blocktime;
                    self.blocktime = blocktime(self.cumtime);

                    NextBlock::pay_all_empty_user(&(lastlightning.shard as usize), &mut self.comittee, &mut self.stkinfo, reward);


                    for i in 0..self.comittee.len() {
                        select_stakers_user(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                    }
                    self.bnum += 1;
                }


                // calculate the reward for this block as a function of the current time and scan either the block or an empty block based on conditions
                let reward = reward(self.cumtime,self.blocktime);
                if !(lastlightning.info.txout.is_empty() && lastlightning.info.stkin.is_empty() && lastlightning.info.stkout.is_empty()) {
                    // let t = Instant::now();
                    // println!("{}",format!("scan stake: {}",t.elapsed().as_millis()).yellow());
                    // let t = Instant::now();
                    // println!("{}",format!("scan: {}",t.elapsed().as_millis()).yellow());
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
                    if let Some(x) = self.lastnonanony.borrow() {
                        if self.bnum%NONCEYNESS == 0 {
                            self.lastnonanony = None;
                            self.gui_sender.send(vec![11]).unwrap();
                        } else {
                            self.lastnonanony = follow(*x,&lastlightning.info.nonanonyout,self.nheight);
                        }
                    }
                    if self.lasttags.par_iter().any(|x| {
                        lastlightning.info.tags.contains(&x)
                    }) {
                        self.lasttags = vec![];
                        self.gui_sender.send(vec![5]).unwrap();
                    }

                    lastlightning.scannonanony(&self.me, &mut self.nmine, &mut self.nheight);
                    lastlightning.scan(&self.me, &mut self.mine, &mut self.reversemine, &mut self.height, &mut self.alltagsever);

                    // let t = Instant::now();
                    lastlightning.scan_as_noone_user(&mut self.stkinfo, &mut self.queue, &mut self.exitqueue, &mut self.comittee, reward, false);
                    // println!("{}",format!("no one: {}",t.elapsed().as_millis()).yellow());

                    self.lastbnum = self.bnum;
                    let mut hasher = Sha3_512::new();
                    hasher.update(m);
                    self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                } else {
                    NextBlock::pay_all_empty_user(&(lastlightning.shard as usize), &mut self.comittee, &mut self.stkinfo, reward);
                }

                for i in 0..self.comittee.len() {
                    select_stakers_user(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                }
                self.bnum += 1;
                


                set_comittee_n_user(lastlightning.shard as usize, &self.comittee, &self.stkinfo);


                // let t = Instant::now();
                // send info to the gui
                // println!("block {} name: {:?}",self.bnum, self.lastname);

                
                self.cumtime += self.blocktime;
                self.blocktime = blocktime(self.cumtime);



                // println!("block reading process done!!!");

                // println!("{}",format!("{}",t.elapsed().as_millis()).bright_yellow());


            }
        }
    }

    /// runs the operations needed for the panic button to work
    fn send_panic_or_stop(&mut self, block: &LightningSyncBlock,mut nheight:usize,save: bool) {
        let mut done = true;
        let mut send = vec![];
        if let Some((a,t,n,fee)) = &mut self.paniced {

            if let Some(x) = &t {
                if block.info.tags.contains(&x.tags[0]) {
                    *t = None;
                } else {
                    if save {
                        let mut x = bincode::serialize(&x).unwrap();
                        x.push(0);
                        send.push(x);
                    }
                    done = false;
                }
            }

            block.scannonanony(a,n,&mut nheight);

            if let Some(nmine) = n {
                let mut amnt = nmine.1;
                if amnt > *fee {
                    amnt -= *fee;
                }

                let mut outs = vec![];
                outs.push((&self.me,Scalar::from(amnt)));
                
                let loc = vec![nmine.0];
                let amnt = vec![nmine.1];
                let inps = amnt.into_iter().map(|x| a.receive_ot(&a.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                let tx = Transaction::spend_ring_nonce(&inps, &outs.iter().map(|x|(x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>(),self.bnum/NONCEYNESS);
                if tx.verify_nonce(self.bnum/NONCEYNESS).is_ok() {
                    let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                    loc.push(2);
                    let tx = tx.polyform(&loc);

                    let mut x = bincode::serialize(&tx).unwrap();
                    x.push(0);
                    send.push(x);
                    done = false;
                }
            }



        }
        for m in send {
            self.send_message(m, TRANSACTION_SEND_TO);

        }
        if done {
            self.paniced = None;
        }
    }

    /// returns the responces of each person you sent it to
    fn send_message(&mut self, message: Vec<u8>, recipients: usize) -> Vec<Vec<u8>> {

        let mut rng = &mut rand::thread_rng();
        self.sendview.shuffle(&mut rng);
        let responces = self.sendview[..cmp::min(recipients,self.sendview.len())].iter().filter_map(|socket| {
            if let Ok(mut stream) =  TcpStream::connect_timeout(socket,CONNECT_TIMEOUT) {
                println!("connected...");
                if stream.set_nonblocking(false).is_ok() {
                    if write_timeout(&mut stream, &message, WRITE_TIMEOUT) {
                        println!("request made...");
                        return read_to_end_timeout(&mut stream, READ_TIMEOUT);
                    }
                } else {
                    println!("can't set nonblocking!");
                }
            }
            return None
        }).collect();

        let mut gm = (self.sendview.len() as u64).to_le_bytes().to_vec();
        gm.push(4);
        self.gui_sender.send(gm).unwrap();


        println!("{}","FINISHED LISTENING".red());
        responces
    }

    /// returns the responces of each person you sent it to and deletes those who are dead from the view
    fn fill_ring(&mut self) {

        let mut rname = self.rname.clone();
        rname.push(114);

        let ring = recieve_ring(&self.rname).unwrap();



        if self.ringsize > 0 {
            let mut rng = &mut rand::thread_rng();
            self.sendview.shuffle(&mut rng);
            let mut memloc = 0usize;
            for socket in self.sendview.iter() {
                if let Ok(mut stream) = TcpStream::connect_timeout(&socket, CONNECT_TIMEOUT) {
                    println!("connected...");
                    if stream.set_nonblocking(false).is_ok() {

                        if write_timeout(&mut stream, &rname, WRITE_TIMEOUT) {
                            println!("request made...");
                            let mut member = [0u8;64]; // using 6 byte buffer
                            while read_timeout(&mut stream, &mut member, READ_TIMEOUT) {
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
                            break
                        } else {
                            println!("can't write to stream!");
                        }
                    } else {
                        println!("can't set nonblocking!");
                    }
                } else {
                    println!("can't connect!");
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
        if self.sendview.len() == 0 {
            println!("your sendview is empty");
        }
        for socket in self.sendview.clone() {
            if let Ok(mut stream) = TcpStream::connect_timeout(&socket, CONNECT_TIMEOUT) {
                if stream.set_nonblocking(false).is_ok() {
                    println!("connected...");
                    let mut m = self.bnum.to_le_bytes().to_vec();
                    m.push(121);
                    if write_timeout(&mut stream, &m, WRITE_TIMEOUT) {
                        println!("request made...");
                        let mut ok = [0;8];
                        if read_timeout(&mut stream,&mut ok, READ_TIMEOUT) {
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
                                self.gui_sender.send(thisbnum).unwrap();
    
                                let mut mymoney = self.mine.iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
                                mymoney.extend(self.nmine.iter().map(|x| x.1).sum::<u64>().to_le_bytes());
                                mymoney.push(0);
                                self.gui_sender.send(mymoney).unwrap();
    
                                // println!("{}",format!("total time: {}ms",tt.elapsed().as_millis()).green().bold());
                                // println!(".");
                            }
                            break
                        }
                    } else {
                        println!("can't write all to stream!");
                    }
                } else {
                    println!("can't set nonblocking!");
                }
            } else {
                println!("this friend is probably busy");
            }
        }
        if self.lasttags.is_empty() {
            self.gui_sender.send(vec![5]).unwrap();
        }
        if self.lastnonanony.is_none() {
            self.gui_sender.send(vec![9]).unwrap();
        }
        self.gui_sender.send([0u8;8].iter().chain(&[7u8]).cloned().collect()).unwrap();
        if self.mine.len() >= ACCOUNT_COMBINE {
            self.gui_sender.send(vec![8]).unwrap();
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
                self.gui_sender.send(gm).unwrap();
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
                        mymoney.extend(self.nmine.iter().map(|x| x.1).sum::<u64>().to_le_bytes());
                        mymoney.push(0);
                        self.gui_sender.send(mymoney).unwrap();

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
                                let ring = recieve_ring(&self.rname).unwrap();


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
                                    self.lasttags.push(tx.tags[0]);
                                    self.send_message(txbin,TRANSACTION_SEND_TO);
                                    println!("{}","==========================\nTRANDACTION SENT\n==========================".bright_yellow().bold());
                                } else {
                                    println!("{}","YOU DIDNT GET THE RING".red().bold());
                                }


                            } else {
                                println!("{}","TRANSACTION INVALID".red().bold());
                            }
                        } else if txtype ==  64 /* ?+1 */ {
                            if let Some(nmine) = self.nmine {
                                let m = Scalar::from(nmine.1) - outs.iter().map(|x| x.1).sum::<Scalar>();
                                let x = outs.len() - 1;
                                outs[x].1 = m;
    
                                
                                let (loc, amnt): (Vec<usize>,Vec<u64>) = vec![nmine].iter().map(|&x|x).unzip();
                                let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                                let tx = Transaction::spend_ring_nonce(&inps, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>(),self.bnum/NONCEYNESS);
                                if tx.verify_nonce(self.bnum/NONCEYNESS).is_ok() {
                                    let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                                    loc.push(2);
                                    let tx = tx.polyform(&loc);
    
                                    let mut txbin = bincode::serialize(&tx).unwrap();
                                    txbin.push(0);
                                    self.lastnonanony = Some(nmine.0);
                                    self.send_message(txbin,TRANSACTION_SEND_TO);
                                    println!("{}","==========================\nTRANDACTION SENT\n==========================".bright_yellow().bold());
                                } else {
                                    println!("{}","TRANSACTION INVALID".red().bold());
                                }
                            } else {
                                println!("{}","YOU HAVE NO NONANONY MONEY".red().bold())
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
                                let rlring = ring.unwrap().iter().map(|x| if let Some(x) = self.rmems.get(x) {x.clone()} else {got_the_ring = false; OTAccount::default()}).collect::<Vec<OTAccount>>();
                                let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                                if got_the_ring && tx.verify().is_ok() {
                                    let tx = tx.polyform(&self.rname);
                                    let mut txbin = bincode::serialize(&tx).unwrap();
                                    txbin.push(0);
                                    self.lasttags.push(tx.tags[0]);
                                    self.send_message(txbin,TRANSACTION_SEND_TO);
                                    println!("{}","==========================\nTRANDACTION SENT\n==========================".bright_yellow().bold());
                                } else {
                                    println!("{}",if !got_the_ring {"YOU DIDNT GET THE RING".red().bold()} else {"TRANSACTION INVALID".red().bold()});
                                }
                            }
                        }
                        println!("{}","done with that".green());


                    } else if istx == u8::MAX /* panic button */ {
                        
                        let fee = u64::from_le_bytes(m.drain(..8).collect::<Vec<_>>().try_into().unwrap());
                        

                        self.paniced = Some((self.me.clone(),None,self.nmine.clone(),fee));
                        let newacc = Account::new(&format!("{}",String::from_utf8_lossy(&m)));

                        if self.mine.len() > 0 {
                            let amnt = self.mine.iter().map(|x| x.1.com.amount.unwrap()).sum::<Scalar>() ;
                            let mut amnt = u64::from_le_bytes(amnt.as_bytes()[..8].to_vec().try_into().unwrap());
                            if amnt > fee {
                                amnt -= fee;
                            }


                            let loc = self.mine.iter().map(|(&x,_)|x).collect::<Vec<_>>();

                            println!("remembered owned accounts");
                            let rname = generate_ring(&loc.iter().map(|&x|x as usize).collect::<Vec<_>>(), &(loc.len() as u16), &self.height);
                            let ring = recieve_ring(&rname).unwrap();

                            println!("made rings");
                            /* you don't use a ring for panics (the ring is just your own accounts) */ 
                            let rlring = ring.iter().map(|&x| self.mine.iter().filter(|(&y,_)| y == x).collect::<Vec<_>>()[0].1.clone()).collect::<Vec<OTAccount>>();
                            
                            

                            let mut outs = vec![];
                            outs.push((&newacc,Scalar::from(amnt)));

                            let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x| (x.0,&x.1)).collect());

                            println!("{:?}",rlring.iter().map(|x| x.com.amount).collect::<Vec<_>>());
                            println!("{:?}",amnt);
                            if tx.verify().is_ok() {
                                let tx = tx.polyform(&rname);
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                txbin.push(0);
                                self.paniced.iter_mut().for_each(|x| {
                                    x.1 = Some(tx.clone())
                                });
                                let responces = self.send_message(txbin.clone(), TRANSACTION_SEND_TO);
                                if !responces.is_empty() {
                                    println!("responce 0 is: {:?}",responces);
                                }
                                println!("{}","==========================\nTRANDACTION SENT\n==========================".bright_yellow().bold());
                            } else {
                                println!("{}","TRANSACTION INVALID".red().bold());
                            }
                        }


                        if let Some(nmine) = self.nmine {
                            let mut amnt = nmine.1;
                            if amnt > fee {
                                amnt -= fee;
                            }


                            let mut outs = vec![];
                            outs.push((&newacc,Scalar::from(amnt)));
                            
                            let (loc, amnt): (Vec<usize>,Vec<u64>) = vec![nmine].iter().map(|&x|x).unzip();
                            let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                            let tx = Transaction::spend_ring_nonce(&inps, &outs.iter().map(|x|(x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>(),self.bnum/NONCEYNESS);
                            if tx.verify_nonce(self.bnum/NONCEYNESS).is_ok() {
                                let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                                loc.push(2);
                                let tx = tx.polyform(&loc);

                                let mut txbin = bincode::serialize(&tx).unwrap();
                                txbin.push(0);
                                self.send_message(txbin,TRANSACTION_SEND_TO);
                                println!("{}","==========================\nTRANDACTION SENT\n==========================".bright_yellow().bold());
                            } else {
                                println!("{}","TRANSACTION INVALID".red().bold());
                            }
                        }

                        self.mine = HashMap::new();
                        self.reversemine = HashMap::new();
                        self.alltagsever = HashSet::new();
                        self.me = newacc;

                        let mut info = bincode::serialize(&
                            vec![
                            bincode::serialize(&self.me.name()).unwrap(),
                            bincode::serialize(&self.me.nonanony_acc().name()).unwrap(),
                            bincode::serialize(&self.me.sk.as_bytes().to_vec()).unwrap(),
                            bincode::serialize(&self.me.vsk.as_bytes().to_vec()).unwrap(),
                            bincode::serialize(&self.me.ask.as_bytes().to_vec()).unwrap(),
                            ]
                        ).unwrap();
                        info.push(254);
                        self.gui_sender.send(info).unwrap();

                    } else if istx == 121 /* y */ { // you clicked sync
                        let mut gm = (self.sendview.len() as u64).to_le_bytes().to_vec();
                        gm.push(4);
                        self.gui_sender.send(gm).unwrap();
                        self.attempt_sync();
                        self.gui_sender.send(vec![self.blocktime as u8,128]).unwrap();


                    } else if istx == 42 /* * */ { // entry address
                        let socket = format!("{}:{}",String::from_utf8_lossy(&m),OUTSIDER_PORT).parse::<SocketAddr>();
                        println!("{:?}",socket);
                        if let Ok(socket) = socket {
                            println!("attempting to connect...");
                            match TcpStream::connect_timeout(&socket, CONNECT_TIMEOUT) {
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
                        }

                    } else if istx == 64 /* @ */ {
                        let mut gm = (self.sendview.len() as u64).to_le_bytes().to_vec();
                        gm.push(4);
                        self.gui_sender.send(gm).unwrap();
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
