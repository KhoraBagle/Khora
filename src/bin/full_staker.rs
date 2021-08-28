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
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::SocketAddr;
use trackable::error::MainError;


use kora::account::*;
use curve25519_dalek::scalar::Scalar;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::time::{Duration, Instant};
use kora::transaction::*;
use curve25519_dalek::ristretto::{CompressedRistretto, RistrettoPoint};
use kora::constants::PEDERSEN_H;
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use kora::bloom::*;
use kora::validation::*;
use kora::ringmaker::*;

use local_ipaddress;

use serde::Serialize;
pub fn hash_to_scalar<T: Serialize> (message: &T) -> Scalar {
    let message = bincode::serialize(message).unwrap();
    let mut hasher = Sha3_512::new();
    hasher.update(&message);
    Scalar::from_hash(hasher)
} /* this is for testing purposes. it is used to check if 2 long messages are identicle */



fn main() -> Result<(), MainError> {
    let matches = app_from_crate!()
        .arg(Arg::with_name("PORT").index(1).required(true))
        .arg(Arg::with_name("PASSWORD").index(2).required(true))
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
    let pswrd = matches.value_of("PASSWORD").unwrap();
    
    let addr: SocketAddr = track_any_err!(format!("{}:{}", local_ipaddress::get().unwrap(), port).parse())?; // gatech
    println!("addr: {:?}",addr);
    println!("pswrd: {:?}",pswrd);


    let max_shards = 64usize; /* this if for testing purposes... there IS NO MAX SHARDS */
    
    // fs::remove_dir_all("blocks").unwrap(); // this would obviously not be used in the final version
    // fs::create_dir_all("blocks").unwrap();


    let executor = track_any_err!(ThreadPoolExecutor::new())?;
    let service = ServiceBuilder::new(addr)
        .logger(logger.clone())
        .finish(executor.handle(), SerialLocalNodeIdGenerator::new()); // everyone is node 0 rn... that going to be a problem? I mean everyone has different ips...
        // .finish(executor.handle(), UnixtimeLocalNodeIdGenerator::new());
        
        // just use a different local node id to represent the comittee?
    let mut node = NodeBuilder::new().logger(logger).finish(service.handle());
    println!("{:?}",node.id());
    if let Some(contact) = matches.value_of("CONTACT_SERVER") {
        println!("contact: {:?}",contact);
        let contact: SocketAddr = track_any_err!(contact.parse())?;
        node.join(NodeId::new(contact, LocalNodeId::new(0)));
    }



    let leader = Account::new(&format!("{}","pig")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
    let mut initial_history = vec![(leader,1u64)];

    // let otheruser = Account::new(&format!("{}","dog")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
    // initial_history.push((otheruser,1u64));

    
    let me = Account::new(&format!("{}",pswrd));
    let validator = me.stake_acc().receive_ot(&me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap(); //make a new account
    let key = validator.sk.unwrap();
    let mut keylocation = HashSet::new();

    History::initialize();
    BloomFile::initialize_bloom_file();
    let bloom = BloomFile::from_keys(1, 2); // everyone has different keys for this

    let mut smine = vec![];
    for i in 0..initial_history.len() {
        if initial_history[i].0 == me.stake_acc().derive_stk_ot(&Scalar::from(initial_history[i].1)).pk.compress() {
            smine.push([i as u64,initial_history[i].1]);
            keylocation.insert(i as u64);
            println!("\n\nhey i guess i founded this crypto!\n\n");
        }

    }


    let (message_tx, message_rx) = mpsc::channel();
    let node = StakerNode {
        inner: node,
        message_rx: message_rx,
        me: me,
        mine: vec![],
        smine: smine, // [location, amount]
        key: key,
        keylocation: keylocation,
        leader: leader,
        overthrown: HashSet::new(),
        votes: vec![0;128],
        stkinfo: initial_history.clone(),
        lastblock: NextBlock::default(),
        queue: (0..max_shards).map(|_|(0..128usize).into_par_iter().map(|x| x%initial_history.len()).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        exitqueue: (0..max_shards).map(|_|(0..128usize).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        comittee: (0..max_shards).map(|_|(0..128usize).into_par_iter().map(|x| x%initial_history.len()).collect::<Vec<usize>>()).collect::<Vec<_>>(),
        lastname: vec![],
        bloom: bloom,
        bnum: 1u64,
        lastbnum: 0u64,
        height: 0u64,
        sheight: 1u64,
        alltagsever: vec![],
        txses: vec![],
        sigs: vec![],
        multisig: false,
        bannedlist: HashSet::new(),
        points: HashMap::new(),
        groupxnonce: 0,
        scalars: HashMap::new(),
        timekeeper: Instant::now(),
        waitingforleader: Instant::now(),
        timeframedone: false,
        stepeven: false,
        clogging: 0,
        emitmessage: Instant::now(),
        laststkgossip: HashSet::new(),
        headshard: 0,
        usurpingtime: Instant::now(),
    };
    executor.spawn(service.map_err(|e| panic!("{}", e)));
    executor.spawn(node);


    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            println!("line sent: {:?}",line);
            let line = if let Ok(line) = line {
                line
            } else {
                break;
            };
            if message_tx.send(line).is_err() {
                println!("message send was error!");
                break;
            }
        }
    });

    track_any_err!(executor.run())?;
    Ok(())
}

struct StakerNode {
    inner: Node<Vec<u8>>,
    me: Account,
    message_rx: mpsc::Receiver<String>,
    mine: Vec<(u64, OTAccount)>,
    smine: Vec<[u64; 2]>, // [location, amount]
    key: Scalar,
    keylocation: HashSet<u64>,
    leader: CompressedRistretto,
    overthrown: HashSet<CompressedRistretto>,
    votes: Vec<i32>,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    lastblock: NextBlock,
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
    multisig: bool,
    bannedlist: HashSet<NodeId>,
    points: HashMap<usize,RistrettoPoint>, // point, supplier
    groupxnonce: u64,
    scalars: HashMap<usize,Scalar>,
    timekeeper: Instant,
    waitingforleader: Instant,
    timeframedone: bool,
    stepeven: bool,
    clogging: u64,
    emitmessage: Instant,
    laststkgossip: HashSet<Vec<u8>>,
    headshard: usize,
    usurpingtime: Instant,
}
impl Future for StakerNode {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;
            print!(".");

            while let Async::Ready(Some(msg)) = track_try_unwrap!(self.inner.poll()) {
                if !self.bannedlist.contains(&msg.id().node()) {
                    let mut m = msg.payload().to_vec();
                    if let Some(mtype) = m.pop() { // dont do unwraps that could mess up a anyone except user
                        self.clogging += 1;
                        // if (mtype == 2) | (mtype == 4) | (mtype == 6) {print!("#{:?}", mtype);}
                        // else {println!("# MESSAGE TYPE: {:?}", mtype);}
                        println!("# MESSAGE TYPE: {:?}", mtype); // i dont do anything with lightning blocks because im a staker
    
    
                        if mtype == 0 {
                            self.txses.push(m[..std::cmp::min(m.len(),10_000)].to_vec());
                        } else if mtype == 1 {
                            let m: Vec<Vec<u8>> = bincode::deserialize(&m).unwrap(); // come up with something better
                            let m = m.into_par_iter().map(|x| bincode::deserialize(&x).unwrap()).collect::<Vec<PolynomialTransaction>>();
    
                            for keylocation in &self.keylocation {
                                let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, &m, &(self.headshard as u16), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo);
                                if m.txs.len() > 0 {
                                    println!("{:?}",m.txs.len());
                                    let mut m = bincode::serialize(&m).unwrap();
                                    m.push(2);
                                    for _ in self.comittee[self.headshard].iter().filter(|&x|*x as u64 == *keylocation).collect::<Vec<_>>() {
                                        self.inner.broadcast(m.clone());
                                        std::thread::sleep(Duration::from_millis(10u64));
                                    }
                                } else if (m.txs.len() == 0) & (m.emptyness.y == Scalar::default()){
                                    self.groupxnonce += 1;
                                    let m = MultiSignature::gen_group_x(&self.key, &self.groupxnonce, &self.bnum).as_bytes().to_vec();// add ,(self.headshard as u16).to_le_bytes().to_vec() to m
                                    let mut m = Signature::sign_message(&self.key, &m, keylocation);
                                    m.push(4);
                                    if !self.comittee[self.headshard].iter().all(|&x|x as u64 != *keylocation) {
                                        self.inner.broadcast(m.clone());
                                    }
                                }
                            }
                            self.waitingforleader = Instant::now();
                            // println!("{:?}",hash_to_scalar(&self.lastblock));
                        } else if mtype == 2 {
                            self.sigs.push(bincode::deserialize(&m).unwrap());
                        } else if mtype == 3 {
                            let lastblock: NextBlock = bincode::deserialize(&m).unwrap();
                            // let mut hasher = Sha3_512::new();
                            // hasher.update(&m);
                            // self.lastblock = bincode::deserialize(&m).unwrap();

    
                            let com = self.comittee.par_iter().map(|x|x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>();
                            println!("names match up: {}",lastblock.last_name == self.lastname);
                            println!("block verified: {}",lastblock.verify(&com[self.headshard], &self.stkinfo).unwrap());
                            if (lastblock.last_name == self.lastname) & lastblock.verify(&com[lastblock.pools[0] as usize], &self.stkinfo).is_ok() {
                                self.lastblock = lastblock;
                                println!("=========================================================\nyay!");
                                // self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();

                                self.lastblock.scan_as_noone(&mut self.stkinfo,&com, &mut self.queue, &mut self.exitqueue, &mut self.comittee, true);
                                self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                                for i in 0..self.comittee.len() {
                                    select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                                }
                                self.lastblock.scan(&self.me, &mut self.mine, &mut self.height, &mut self.alltagsever);
                                self.lastblock.scanstk(&self.me, &mut self.smine, &mut self.sheight, &com[self.headshard]);
    
                                let lightning = bincode::serialize(&self.lastblock.tolightning()).unwrap();
                                if (self.lastblock.txs.len() > 0) | (self.bnum - self.lastbnum > 4) {
                                    println!("saving block...");
                                    let mut f = File::create(format!("blocks/b{}",self.lastblock.bnum)).unwrap();
                                    f.write_all(&m).unwrap(); // writing doesnt show up in blocks in vs code immediatly
                                    self.lastbnum = self.bnum;
                                }
                                let mut hasher = Sha3_512::new();
                                hasher.update(lightning);
                                self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                                self.bnum += 1;
                                

                                
    
                                if self.keylocation.contains(&(self.exitqueue[self.headshard][0] as u64)) | self.keylocation.contains(&(self.exitqueue[self.headshard][1] as u64)) {
                                    /* broadcast the block to the outside world*/
                                }
    

                                if self.lastblock.validators.len() == 0 {
                                    self.votes = self.votes.iter().zip(self.comittee[self.headshard].iter()).map(|(z,&x)| z - self.lastblock.emptyness.pk.iter().filter(|&&y| y == x as u64).count() as i32).collect::<Vec<_>>();
                                } else {
                                    self.votes = self.votes.iter().zip(self.comittee[self.headshard].iter()).map(|(z,&x)| z + self.lastblock.validators.iter().filter(|y| y.pk == x as u64).count() as i32).collect::<Vec<_>>();
                                }
                                
                                /* LEADER CHOSEN BY VOTES */
                                self.leader = self.stkinfo[*self.comittee[self.headshard].iter().zip(self.votes.iter()).max_by_key(|(_,&y)| y).unwrap().0].0;
                                /* LEADER CHOSEN BY VOTES */
                                

                                self.timeframedone = true;
                                self.waitingforleader = Instant::now();
                                self.timekeeper = Instant::now();
                                self.usurpingtime = Instant::now();
                            }
                            // println!("{:?}",hash_to_scalar(&self.lastblock));
                        } else if mtype == 4 {
                            if let Some(pk) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                                let pk = pk as usize;
                                if !self.comittee[self.headshard].par_iter().all(|x| x!=&pk) {
                                    self.points.insert(pk,CompressedRistretto(m.try_into().unwrap()).decompress().unwrap());
                                }
                            }
                        } else if mtype == 5 {
                            let xt = CompressedRistretto(m.try_into().unwrap());
                            let mut mess = self.leader.as_bytes().to_vec();
                            mess.extend(&self.lastname);
                            mess.extend(&(self.headshard as u16).to_le_bytes().to_vec());
                            // println!("from the me: {:?}",mess);
                            println!("you're trying to send a scalar!");
                            for keylocation in self.keylocation.iter() {
                                let mut m = Signature::sign_message(&self.key, &MultiSignature::try_get_y(&self.key, &self.groupxnonce, &self.bnum, &mess, &xt).as_bytes().to_vec(), keylocation);
                                m.push(6u8);
                                if !self.comittee[self.headshard].iter().all(|&x| !self.keylocation.contains(&(x as u64))) {
                                    self.inner.broadcast(m.clone());
                                }
                            }
                            self.waitingforleader = Instant::now();
                        } else if mtype == 6 {
                            println!("someone's trying to send you a scalar!");
                            if let Some(pk) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                                let pk = pk as usize;
                                println!("someone's REALLY trying to send you a scalar!");
                                if !self.comittee[self.headshard].par_iter().all(|x| x!=&pk) {
                                    self.scalars.insert(pk,Scalar::from_bits(m.try_into().unwrap()));
                                }
                            }
                        } else if mtype == 129 {
                            self.laststkgossip.insert(m);
                        } else if mtype == u8::MAX {
                            println!("address:              {:?}",self.inner.plumtree_node().id());
                            println!("eager push pears:     {:?}",self.inner.plumtree_node().eager_push_peers());
                            println!("lazy push pears:      {:?}",self.inner.plumtree_node().lazy_push_peers());
                            println!("active view:          {:?}",self.inner.hyparview_node().active_view());
                            println!("passive view:         {:?}",self.inner.hyparview_node().passive_view());
                            
                            
                            // let mut s = Sha3_512::new();
                            // s.update(&bincode::serialize(&self.inner.plumtree_node().id()).unwrap());
                            // s.update(&bincode::serialize(&self.bnum).unwrap());
                            // let s = bincode::serialize( // is bincode ok for things phones have to read???
                            //     &(Signature::sign(&self.key, &mut s,&self.keylocation.iter().next().unwrap()),
                            //     self.inner.hyparview_node().id().address(),
                            //     self.bnum,)
                            // ).unwrap();
                            // let (a,b,c): (Signature, SocketAddr, u64) = bincode::deserialize(&s).unwrap();
    
    
    
    
    
                            let mut y = m[..8].to_vec();
                            let mut x = History::get_raw(&u64::from_le_bytes(y.clone().try_into().unwrap())).to_vec();
                            x.append(&mut y);
                            x.push(254);
                            self.inner.dm(x,&vec![msg.id().node()],false);
                        }
                    }
                }
                did_something = true;
            }










/*
send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- 
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000a!
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000b!
  send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   ------- 
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000a!

               VVVVVVVVVVVVVVVVVV split stake VVVVVVVVVVVVVVVVVV
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000a!

                  VVVVVVVVVVV pump up the height VVVVVVVVVVV
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigmnjimdgmpelmdoehiemiefmhinffkcnbmkjofflhfcpbcamfhheknjkibbcooeccgfemcpbnfommaiefmllkeekmghjokbhjepfgnfeilgjkipokjmfffggckekhpbef10000000ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigmnjimdgmpelmdoehiemiefmhinffkcnbmkjofflhfcpbcamfhheknjkibbcooeccgfemcpbnfommaiefmllkeekmghjokbhjepfgnfeilgjkipokjmfffggckekhpbef10000000ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigmnjimdgmpelmdoehiemiefmhinffkcnbmkjofflhfcpbcamfhheknjkibbcooeccgfemcpbnfommaiefmllkeekmghjokbhjepfgnfeilgjkipokjmfffggckekhpbef10000000a!


   VVVVVVVVVVVVVVV send from non stake (!) to non stake (gf...ob) VVVVVVVVVVVVVVV
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000!!

*/






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
            while let Async::Ready(Some(m)) = self.message_rx.poll().expect("Never fails") {
                if m.len() > 0 {
                    println!("# MESSAGE (sent): {:?}", m);
                    let mut m = str::to_ascii_lowercase(&m).as_bytes().to_vec();
                    let istx = m.pop().unwrap();
                    if istx == 33 /* ! */ {
                        let txtype = m.pop().unwrap();
                        let mut outs = vec![];
                        while m.len() > 0 {
                            let mut pks = vec![];
                            for _ in 0..3 { // read the pk address
                                let h1 = m.par_drain(..32).collect::<Vec<_>>().par_iter().map(|x| (x-97)).collect::<Vec<_>>();
                                let h2 = m.par_drain(..32).collect::<Vec<_>>().par_iter().map(|x| (x-97)*16).collect::<Vec<_>>();
                                pks.push(CompressedRistretto(h1.into_par_iter().zip(h2).map(|(x,y)|x+y).collect::<Vec<u8>>().try_into().unwrap()));
                            }
                            let x: [u8;8] = m.par_drain(..8).map(|x| (x-48)).collect::<Vec<_>>().try_into().unwrap();
                            // println!("hi {:?}",x);
                            let x = u64::from_le_bytes(x);
                            println!("amounts {:?}",x);
                            // println!("ha {:?}",1u64.to_le_bytes());
                            let amnt = Scalar::from(x);
                            let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                            outs.push((recv,amnt));
                        }

                        let mut txbin: Vec<u8>;
                        if txtype == 33 /* ! */ {
                            let (loc, acc): (Vec<u64>,Vec<OTAccount>) = self.mine.par_iter().map(|x|(x.0 as u64,x.1.clone())).unzip();

                            let rname = generate_ring(&loc.par_iter().map(|x|*x as usize).collect::<Vec<_>>(), &5, &self.height);
                            let ring = recieve_ring(&rname);
                            println!("ring: {:?}",ring);
                            println!("mine: {:?}",acc.iter().map(|x|x.pk.compress()).collect::<Vec<_>>());
                            println!("ring: {:?}",ring.iter().map(|x|OTAccount::summon_ota(&History::get(&x)).pk.compress()).collect::<Vec<_>>());
                            let mut rlring = ring.into_iter().map(|x| {
                                let x = OTAccount::summon_ota(&History::get(&x));
                                if acc.par_iter().all(|a| a.pk != x.pk) {
                                    println!("not mine!");
                                    x
                                } else {
                                    println!("mine!");
                                    acc.par_iter().filter(|a| a.pk == x.pk).collect::<Vec<_>>()[0].to_owned()
                                }
                            }).collect::<Vec<OTAccount>>();
                            println!("ring len: {:?}",rlring.len());
                            let me = self.me;
                            rlring.par_iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x.clone()) {*x = y;});
                            let tx = Transaction::spend_ring(&rlring, &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                            tx.verify().unwrap();
                            let tx = tx.polyform(&rname);
                            tx.verify().unwrap();
                            txbin = bincode::serialize(&tx).unwrap();
                        } else {
                            let (loc, amnt): (Vec<u64>,Vec<u64>) = self.smine.par_iter().map(|x|(x[0] as u64,x[1].clone())).unzip();
                            let i = txtype as usize - 97usize;
                            let b = self.me.derive_stk_ot(&Scalar::from(amnt[i]));
                            let tx = Transaction::spend_ring(&vec![self.me.receive_ot(&b).unwrap()], &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                            tx.verify().unwrap();
                            println!("stkinfo: {:?}",self.stkinfo);
                            println!("me pk: {:?}",self.me.receive_ot(&b).unwrap().pk.compress());
                            println!("loc: {:?}",loc);
                            println!("amnt: {:?}",amnt);
                            let tx = tx.polyform(&loc[i].to_le_bytes().to_vec());
                            tx.verifystk(&self.stkinfo).unwrap();
                            txbin = bincode::serialize(&tx).unwrap();
                        }
                        txbin.push(0);

                        println!("----------------------------------------------------------------\n{:?}",txbin);
                        self.inner.broadcast(txbin);
                    } else if istx == 42 /* * */ { // ips to talk to
                        // 192.168.000.101:09876 192.168.000.101:09875*
                        let addrs = String::from_utf8_lossy(&m);
                        let addrs = addrs.split(" ").collect::<Vec<_>>().par_iter().map(|x| NodeId::new(x.parse::<SocketAddr>().unwrap(), LocalNodeId::new(0))).collect::<Vec<_>>();
                        self.inner.dm(vec![],&addrs,true);
                    } else if istx == 105 /* i */ {
                        println!("\nmy name:\n---------------------------------------------\n{:?}\n",self.me.name());
                        println!("\nmy addr plumtree:\n---------------------------------------------\n{:?}\n",self.inner.plumtree_node().id());
                        println!("\nmy addr hyparview:\n---------------------------------------------\n{:?}\n",self.inner.hyparview_node().id());
                        println!("\nmy staker name:\n---------------------------------------------\n{:?}\n",self.me.stake_acc().name());
                        let scalarmoney = self.mine.iter().map(|x|self.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>();
                        println!("\nmy scalar money:\n---------------------------------------------\n{:?}\n",scalarmoney);
                        let moniez = u64::from_le_bytes(scalarmoney.as_bytes()[..8].try_into().unwrap());
                        println!("\nmy money:\n---------------------------------------------\n{:?}\n",moniez);
                        println!("\nmy money locations:\n---------------------------------------------\n{:?}\n",self.mine.iter().map(|x|x.0 as u64).collect::<Vec<_>>());
                        let stake = self.smine.iter().map(|x|x[1]).collect::<Vec<_>>();
                        println!("\nmy stake:\n---------------------------------------------\n{:?}\n",stake);
                        println!("\nheight:\n---------------------------------------------\n{:?}\n",self.height);
                        println!("\nsheight:\n---------------------------------------------\n{:?}\n",self.sheight);
                    } else if istx == 98 /* b */ {
                        println!("\nlast block:\n---------------------------------------------\n{:#?}\n",self.lastblock);
                    } else if istx == 97 /* a */ { // 9876 9875a   (just input the ports, only for testing on a single computer)
                        let addrs = String::from_utf8_lossy(&m);
                        let addrs = addrs.split(" ").collect::<Vec<_>>().par_iter().map(|x| NodeId::new( format!("{}:{}", local_ipaddress::get().unwrap(), x).parse::<SocketAddr>().unwrap(), LocalNodeId::new(0))).collect::<Vec<_>>();

                        self.inner.dm(vec![],&addrs,true);
                    }
                }
                did_something = true;
            }
























/*_________________________________________________________________________________________________________
LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF |||||||||||||
||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF
LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF |||||||||||||
||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF
LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF |||||||||||||
||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF
LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF |||||||||||||
||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF
*/
            // if self.headshard != 0 { // that tests shard usurption
            if (self.waitingforleader.elapsed().as_secs() > 5) & !self.timeframedone {
                self.waitingforleader = Instant::now();
                /* change the leader, also add something about only changing the leader if block is free */

                self.overthrown.insert(self.leader);
                self.leader = self.stkinfo[*self.comittee[0].iter().zip(self.votes.iter()).max_by_key(|(&x,&y)| {
                    let candidate = self.stkinfo[x];
                    if self.overthrown.contains(&candidate.0) {
                        i32::MIN
                    } else {
                        y
                    }
                }).unwrap().0].0;
            }
            if !self.keylocation.iter().all(|x|self.stkinfo[*x as usize].0 != self.leader) & ( (self.sigs.len() > 85) | ( (self.sigs.len() > 64) & (self.timekeeper.elapsed().as_secs() > 30) ) ) {
                let lastblock = NextBlock::finish(&self.key, &self.keylocation.iter().next().unwrap(), &self.sigs.drain(..).collect::<Vec<_>>(), &self.comittee[self.headshard].par_iter().map(|x|*x as u64).collect::<Vec<u64>>(), &(self.headshard as u16), &self.bnum, &self.lastname, &self.stkinfo);

                if lastblock.validators.len() != 0 {
                    self.lastblock = lastblock;

                    let mut m = bincode::serialize(&self.lastblock).unwrap();
                    let mut l = bincode::serialize(&self.lastblock.tolightning()).unwrap();
    
                    self.sigs = vec![];

                    let mut hasher = Sha3_512::new();
                    hasher.update(&l);
                    // self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                    // println!("{:?}",hash_to_scalar(&self.lastblock));


    

                    l.push(7u8);
                    self.inner.broadcast(l);
                    m.push(3u8);
                    self.inner.broadcast(m);


                    

                    println!("made a block with {} transactions!",self.lastblock.txs.len());
                    did_something = true;
                } else {
                    println!("failed to make a block :(");
                    did_something = false;
                }

                self.timekeeper = Instant::now();
            }
            if !self.stepeven & !self.keylocation.iter().all(|x|self.stkinfo[*x as usize].0 != self.leader) & (self.multisig == true) & (self.timekeeper.elapsed().as_secs() > 1) & (self.comittee[self.headshard].iter().filter(|&x|self.points.contains_key(x)).count() > 85) {
                // should prob check that validators are accurate here?
                let points = self.points.par_iter().map(|x| *x.1).collect::<Vec<_>>();
                let mut m = MultiSignature::sum_group_x(&points).as_bytes().to_vec();
                m.push(5u8);
                self.inner.broadcast(m);
                // self.points = HashMap::new();
                self.points.insert(usize::MAX,MultiSignature::sum_group_x(&points).decompress().unwrap());
                self.stepeven = true;
                did_something = true;

            }
            let k = self.scalars.keys().collect::<HashSet<_>>();
            // println!("{:?}",self.stepeven & self.points.get(&usize::MAX).is_some() & !self.keylocation.iter().all(|x|self.stkinfo[*x as usize].0 != self.leader) & (self.multisig == true) & (self.timekeeper.elapsed().as_secs() > 2));
            // println!("but also {:?}", self.comittee[0].iter().filter(|x| k.contains(x)).count());
            // println!("but also {:?}", k);
            if self.stepeven & self.points.get(&usize::MAX).is_some() & !self.keylocation.iter().all(|x|self.stkinfo[*x as usize].0 != self.leader) & (self.multisig == true) & (self.timekeeper.elapsed().as_secs() > 2) & (self.comittee[self.headshard].iter().filter(|x| k.contains(x)).count() > 85) {
                self.multisig = false; // should definitely check that validators are accurate here
                // this is for if everyone signed... really > 0.5 or whatever... 
                
                let sumpt = self.points.remove(&usize::MAX).unwrap();

                let keys = self.points.clone();
                let mut keys = keys.keys().collect::<Vec<_>>();
                let mut s = Sha3_512::new();
                let mut m = self.leader.as_bytes().to_vec();
                m.extend(&self.lastname);
                m.extend(&(self.headshard as u16).to_le_bytes().to_vec());
                s.update(&m);
                s.update(sumpt.compress().as_bytes());
                let e = Scalar::from_hash(s);
                // println!("keys: {:?}",keys);
                let k = keys.len();
                keys.retain(|&x| (self.points[x] + e*self.stkinfo[*x].0.decompress().unwrap() == self.scalars[x]*PEDERSEN_H()) & self.comittee[self.headshard].contains(x));
                if k == keys.len() {

                    let failed_validators = vec![];
                    let mut lastblock = NextBlock::default();
                    lastblock.bnum = self.bnum;
                    lastblock.emptyness = MultiSignature{x: sumpt.compress(), y: MultiSignature::sum_group_y(&self.scalars.values().map(|x| *x).collect::<Vec<_>>()), pk: failed_validators};
                    lastblock.last_name = self.lastname.clone();
                    lastblock.pools = vec![self.headshard as u16];
    
                    
                    let m = vec![BLOCK_KEYWORD.to_vec(),(self.headshard as u16).to_le_bytes().to_vec(),self.bnum.to_le_bytes().to_vec(),self.lastname.clone(),bincode::serialize(&lastblock.emptyness).unwrap().to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
                    let mut s = Sha3_512::new();
                    s.update(&m);
                    let leader = Signature::sign(&self.key, &mut s,&self.keylocation.iter().next().unwrap());
                    lastblock.leader = leader;
    
    
                    let mut m = bincode::serialize(&lastblock).unwrap();
                    let mut l = bincode::serialize(&lastblock.tolightning()).unwrap();
                    
                    // self.lastblock = lastblock;
                    // let mut hasher = Sha3_512::new();
                    // hasher.update(&l);
                    println!("sending off block {}!!!",lastblock.bnum);
                    m.push(3u8);
                    self.inner.broadcast(m);
    
                    l.push(7u8);
                    self.inner.broadcast(l);
    
                    self.points = HashMap::new();
                    self.scalars = HashMap::new();
                    self.sigs = vec![];
    
                    // let m = bincode::serialize(&self.lastblock).unwrap();
                    // let mut hasher = Sha3_512::new();
                    // hasher.update(&m);
                    // self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                    // self.bnum += 1;
                    
                    self.stepeven = false;
                    self.timekeeper = Instant::now();

                } else { // need an extra round to weed out liers
                    let points = keys.iter().map(|x| self.points[x]).collect::<Vec<_>>();
                    let p = MultiSignature::sum_group_x(&points);
                    self.points.insert(usize::MAX,p.decompress().unwrap());
                    let mut m = p.as_bytes().to_vec();
                    m.push(5u8);
                    self.inner.broadcast(m);
                    self.points = keys.iter().map(|&x| (*x,self.points[x])).collect::<HashMap<_,_>>();
                    self.scalars = HashMap::new();

                    self.timekeeper -= Duration::from_secs(1);
                }
                did_something = true;

            }
            if !self.keylocation.iter().all(|x| self.stkinfo[*x as usize].0 != self.leader) & (self.timekeeper.elapsed().as_secs() > 10) { // make this a floating point function for variable time
                self.sigs = vec![];
                self.points = HashMap::new();
                self.scalars = HashMap::new();
                let mut m = bincode::serialize(&self.txses).unwrap();
                m.push(1u8);
                self.inner.broadcast(m);
                self.txses = vec![];
                self.timekeeper = Instant::now();
                if self.txses.len() == 0 {self.multisig = true;}
                did_something = true;
            }
            // }








            if self.usurpingtime.elapsed().as_secs() > 300 { // this will be much larger
                self.timekeeper = self.usurpingtime;
                self.usurpingtime = Instant::now();
                self.headshard += 1;
                // CHANGE IMPORTANT SHARD
                // ALSO ADD IF RECIEVE BLOCK OF KIND TRUST IT AND OVERIDE SHARD 0 BLOCKS

                // RecieveBlock class acts like history
                // RecieveBlock::get(), RecieveBlock::replace(/* removes future too */)
                // add get_name() function to block

                // if block_n.last_block == RecieveBlock::(n-1).get_name() & block.verify() & block.shard.iter().min() > self.shard {self.shard = block.shard.iter().min()}

            }


/*____________________________________________________________________________________________________________________________________________________________________________________________________________________________
 RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION |
 RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION |
 RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION |
 RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION |
 RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION |
*/////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
            if self.emitmessage.elapsed().as_secs() > 300 {
                self.emitmessage = Instant::now();
                let mut m = hash_to_scalar(&self.stkinfo).as_bytes().to_vec();
                if self.laststkgossip.contains(&m) & (self.clogging < 3000) { // 10 messages per second
                    m.push(129u8);
                    self.inner.broadcast(m);
                    let mut m = Signature::sign_message(&self.key, &bincode::serialize(&self.inner.id().address()).unwrap(), &self.keylocation.iter().next().unwrap());
                    m.push(130u8);
                    self.inner.broadcast(m);
                }
                self.laststkgossip = HashSet::new();
                self.clogging = 0;
            }
        }
        Ok(Async::NotReady)
    }
}