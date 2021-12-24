use curve25519_dalek::ristretto::{CompressedRistretto};
use curve25519_dalek::scalar::Scalar;
use crate::account::*;
use crate::vechashmap::{VecHashMap, follow};
use rayon::prelude::*;
use crate::transaction::*;
use std::borrow::Borrow;
use std::convert::TryInto;
use std::iter::FromIterator;
use std::net::SocketAddr;
use crate::bloom::BloomFile;
use rand::{thread_rng};
use sha3::{Digest, Sha3_512};
use ahash::AHasher;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::hash::Hasher;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet, VecDeque};
use crate::constants::PEDERSEN_H;
use crate::vechashmap::*;
use std::io::{Seek, SeekFrom, BufReader};//, BufWriter};
use std::time::Duration;


pub static VERSION: &str = "v0.96";
pub static KHORA_WEBSITE: &str = "https://khora.info";


/// the number of validators in the comittee, 128
pub const NUMBER_OF_VALIDATORS: usize = 3;
/// the number of validators who need to sign for a block to be approved, 2/3
pub const SIGNING_CUTOFF: usize = 2*NUMBER_OF_VALIDATORS/3;
/// the amount of time in advance people know they will be a validator, 128
pub const QUEUE_LENGTH: usize = 10;
/// the number of people who leave the comittee per block, 2
pub const REPLACERATE: usize = 2;
/// the fraction of money you use for failing to do your duties as a comittee member
pub const PUNISHMENT_FRACTION: u64 = 1000;
/// user 0: the first person in the blockchain
pub const PERSON0: CompressedRistretto = CompressedRistretto([46, 235, 227, 188, 55, 53, 9, 126, 167, 207, 202, 101, 150, 150, 172, 207, 209, 208, 211, 52, 47, 206, 19, 115, 199, 189, 202, 10, 56, 220, 138, 55]);
/// minimum stake
pub const MINSTK: u64 = 1_000_000;
/// total money ever produced
pub const TOTAL_KHORA: f64 = (u64::MAX/2u64) as f64;
/// bloom file for stakers
pub const STAKER_BLOOM_NAME: &'static str = "all_tags";
/// bloom file for stakers size
pub const STAKER_BLOOM_SIZE: usize =  1_000_000_000*4;
/// bloom file for stakers hashes
pub const STAKER_BLOOM_HASHES: u8 = 13;
/// if you have to many tx, you should combine them
pub const ACCOUNT_COMBINE: usize = 10;
/// read timeout for stream in millis
pub const READ_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
/// write timeout for stream in millis
pub const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
/// how often the nonce is replaced for visible transactions
pub const NONCEYNESS: u64 = 100;
/// how many comittees you keep track of over time
pub const LONG_TERM_SHARDS: usize = 2;
/// when to announce you're about to be in the comittee or how far in advance you can no longer serve as leader
pub const EXIT_TIME: usize = REPLACERATE*5;
/// amount of seconds to wait before initiating shard takeover
pub const USURP_TIME: u64 = 3600;
/// the default port
pub const DEFAULT_PORT: u16 = 8334;
/// the outsider port
pub const OUTSIDER_PORT: u16 = 8335;
/// the number of nodes to send each message to as a user
pub const TRANSACTION_SEND_TO: usize = 1;


/// calculates the reward for the current block
pub fn reward(cumtime: f64, blocktime: f64) -> f64 {
    (1.0/(1.653439E-6*cumtime + 1.0) - 1.0/(1.653439E-6*(cumtime + blocktime) + 1.0))*TOTAL_KHORA
}
/// calculates the amount of time the current block takes to be created
pub fn blocktime(cumtime: f64) -> f64 {
    // 60f64/(6.337618E-8f64*cumtime+2f64).ln()
    10.0
    // 30.0
}

#[derive(Default, Clone, Serialize, Deserialize, Eq, Hash, Debug)]
/// the information on the transactions made that is saved in lightning blocks
/// in a minimalist setting where validators dont ever need to sync anyone, they only need to save their history object and the stake state at the current time
pub struct Syncedtx{
    /// this should already be sorted
    pub stkout: Vec<usize>,
    pub stkin: Vec<(usize,u64)>,
    pub stknew: Vec<(CompressedRistretto,u64)>,
    /// this should already be sorted
    pub nonanonyout: Vec<usize>,
    pub nonanonyin: Vec<(usize,u64)>,
    pub nonanonynew: Vec<(CompressedRistretto,u64)>,
    pub txout: Vec<OTAccount>,
    pub tags: Vec<CompressedRistretto>,
    pub fees: u64,
}

impl PartialEq for Syncedtx {
    fn eq(&self, other: &Self) -> bool {
        self.stkout == other.stkout && self.stkin == other.stkin && self.txout == other.txout && self.tags == other.tags && self.fees == other.fees && self.nonanonyin == other.nonanonyin && self.nonanonyout == other.nonanonyout && self.nonanonynew == other.nonanonynew
    }
}

impl Syncedtx {
    /// distills the important information from a group of transactions
    /// this does return the stkout and nonanonyout sorted
    pub fn from(txs: &Vec<PolynomialTransaction>, nonanonyinfo: &VecHashMap<CompressedRistretto,u64>, stkinfo: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>)->Syncedtx {

        let ((tags,fees), (inputs, outputs)): ((Vec<_>,Vec<_>),(Vec<_>,Vec<Vec<_>>)) = txs.into_par_iter().map(|x|
            ((if x.inputs.last() == Some(&0) {Some(x.tags.clone())} else {None},x.fee),(x.inputs.clone(),x.outputs.clone()))
        ).unzip();
        let tags = tags.into_par_iter().filter_map(|x|x).flatten().collect::<Vec<_>>();
        let outputs = outputs.into_par_iter().flatten().collect::<Vec<_>>();
        let fees = fees.into_par_iter().sum::<u64>();

        let (txout, (stkin,nonanonyin)): (Vec<_>,(Vec<_>,Vec<_>)) = outputs.into_par_iter().map(|x| {
            if let Ok(z) = stakereader_acc().read_ot(&x) {
                (None,(Some((z.pk.compress(),u64::from_le_bytes(z.com.amount.unwrap().as_bytes()[..8].try_into().unwrap()))),None))
            } else if let Ok(z) = nonanonyreader_acc().read_ot(&x) {
                (None,(None,Some((z.pk.compress(),u64::from_le_bytes(z.com.amount.unwrap().as_bytes()[..8].try_into().unwrap())))))
            } else {
                (Some(x),(None,None))
            }
        }).unzip();
        let txout = txout.into_par_iter().filter_map(|x|x).collect::<Vec<_>>();
        let mut stkin = stkin.into_par_iter().filter_map(|x|x).collect::<Vec<_>>();
        let mut nonanonyin = nonanonyin.into_par_iter().filter_map(|x|x).collect::<Vec<_>>();

        let (stkout,nonanonyout): (Vec<_>,Vec<_>) = inputs.into_par_iter().filter_map(|x| {
            if x.last() == Some(&1) {
                Some((Some(x.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap()) as usize).collect::<Vec<_>>()),None))
            } else if x.last() == Some(&2) {
                Some((None,Some(x.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap()) as usize).collect::<Vec<_>>())))
            } else {None}
        }).unzip();
        let mut stkout = stkout.into_par_iter().filter_map(|x|x).flatten().collect::<Vec<_>>();
        stkout.sort();
        let mut nonanonyout = nonanonyout.into_par_iter().filter_map(|x|x).flatten().collect::<Vec<_>>();
        nonanonyout.sort();




        let s = stkin.clone();
        stkin.par_iter_mut().enumerate().for_each(|(i,x)| {
            x.1 += s.par_iter().take(i).filter_map(|y| {
                if y.0 == x.0 {
                    Some(y.1)
                } else {
                    None
                }
            }).sum::<u64>();
        });
        stkin = stkin.par_iter().rev().enumerate().filter_map(|(i,&x)| {
            if s.par_iter().rev().take(i).all(|y| y.0 != x.0) {
                Some(x)
            } else {
                None
            }
        }).collect();
        let (stknew, stkin): (Vec<_>,Vec<_>) = stkin.par_iter().map(|x| {
            if let Some(z) = stkinfo.contains(&x.0) {
                (None,Some((z,x.1)))
            } else {
                (Some(x),None)
            }
        }).unzip();
        let mut stkin = stkin.par_iter().filter_map(|x|x.as_ref()).copied().collect::<Vec<_>>();
        let stknew = stknew.par_iter().filter_map(|x|x.copied()).collect::<Vec<_>>();
        for (x,a) in &mut stkin {
            if let Some((z,_)) = stkout.par_iter().enumerate().find_first(|(_,y)| **y == *x) {
                stkout.remove(z);
            } else {
                *a = stkinfo.vec[*x].1.0 + *a;
            }
        }




        let s = nonanonyin.clone();
        nonanonyin.par_iter_mut().enumerate().for_each(|(i,x)| {
            x.1 += s.par_iter().take(i).filter_map(|y| {
                if y.0 == x.0 {
                    Some(y.1)
                } else {
                    None
                }
            }).sum::<u64>();
        });
        nonanonyin = nonanonyin.par_iter().rev().enumerate().filter_map(|(i,&x)| {
            if s.par_iter().rev().take(i).all(|y| y.0 != x.0) {
                Some(x)
            } else {
                None
            }
        }).collect();
        let (nonanonynew, nonanonyin): (Vec<_>,Vec<_>) = nonanonyin.par_iter().map(|x| {
            if let Some(z) = nonanonyinfo.contains(&x.0) {
                (None,Some((z,x.1)))
            } else {
                (Some(x),None)
            }
        }).unzip();
        let mut nonanonyin = nonanonyin.par_iter().filter_map(|x|x.as_ref()).copied().collect::<Vec<_>>();
        let nonanonynew = nonanonynew.par_iter().filter_map(|x|x.copied()).collect::<Vec<_>>();
        
        
        for (x,a) in &mut nonanonyin {
            if let Some((z,_)) = nonanonyout.par_iter().enumerate().find_first(|(_,y)| **y == *x) {
                nonanonyout.remove(z);
            } else {
                *a = nonanonyinfo.vec[*x].1 + *a;
            }
        }

        // println!("outn {:?}",nonanonyout);
        // println!("inn {:?}",nonanonyin);
        // println!("newn {:?}",nonanonynew);
        // println!("outs {:?}",stkout);
        // println!("ins {:?}",stkin);
        // println!("news {:?}",stknew);
        // println!("newt {:?}",txout.iter().map(|x| x.pk.compress()).collect::<Vec<_>>());


        Syncedtx{stkout,stkin,stknew,nonanonyout,nonanonynew,nonanonyin,txout,tags,fees}
    }

    /// the message block creaters sign so even lightning blocks can be verified
    pub fn to_sign(txs: &Vec<PolynomialTransaction>,nonanonyinfo:&VecHashMap<CompressedRistretto, u64>,stkinfo:&VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>)->Vec<u8> {
        bincode::serialize(&Syncedtx::from(txs,nonanonyinfo,stkinfo)).unwrap()
    }

    /// checks the info is empty
    pub fn is_empty(&self)->bool {
        self.txout.is_empty() && self.stkout.is_empty() && self.stkin.is_empty() && self.nonanonyout.is_empty() && self.nonanonyin.is_empty() && self.tags.is_empty()
    }
}


#[derive(Default, Clone, Eq, Serialize, Deserialize, Hash, Debug)]
/// this is not in use. signs a message as a comittee member
pub struct ValidatorSignature{
    pub c: Scalar,
    pub r: Scalar,
    pub pk: u8,
}

impl PartialEq for ValidatorSignature {
    fn eq(&self, other: &Self) -> bool {
        self.c == other.c && self.r == other.r && self.pk == other.pk
    }
}

impl ValidatorSignature {
    /// signs a message as a validator. the message is input as a hash state
    pub fn sign(key: &Scalar, message: &mut Sha3_512, location: &u8) -> ValidatorSignature {
        let mut csprng = thread_rng();
        let a = Scalar::random(&mut csprng);
        message.update((a*PEDERSEN_H()).compress().to_bytes());
        let c = Scalar::from_hash(message.to_owned());
        ValidatorSignature{c, r: (a - c*key), pk: *location}
    }

    /// converts a signature made as a comittee member to one made as a generic staker
    pub fn to_signature(&self, validator_pool: &Vec<usize>) -> Signature {
        Signature {
            c: self.c,
            r: self.r,
            pk: validator_pool[self.pk as usize],
        }
    }
}
#[derive(Default, Clone, Eq, Serialize, Deserialize, Hash, Debug)]
/// a signature. This does not include any information on the message being signed
pub struct Signature{
    pub c: Scalar,
    pub r: Scalar,
    pub pk: usize,
}

impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.c == other.c && self.r == other.r && self.pk == other.pk
    }
}

impl Signature {
    /// replaces the public key location with the location in the comittee to save 7 bytes (not used because it's messier with little reward)
    /// this hasn't actually been implimented in the blocks
    pub fn to_validator_signature(&self, validator_pool: &Vec<usize>) -> ValidatorSignature {
        ValidatorSignature{
            c: self.c,
            r: self.r,
            pk: validator_pool.par_iter().enumerate().filter_map(|(i,&pk)| {
                if pk == self.pk {
                    Some(i)
                } else {
                    None
                }
            }).collect::<Vec<_>>()[0] as u8
        }
    }

    /// signs a message given as a Sha3_512 state with your key
    pub fn sign(key: &Scalar, message: &mut Sha3_512, location: &usize) -> Signature { // the inputs are the hashed messages you are checking for signatures on because it's faster for many messages.
        let mut csprng = thread_rng();
        let a = Scalar::random(&mut csprng);
        message.update((a*PEDERSEN_H()).compress().to_bytes());
        let c = Scalar::from_hash(message.to_owned());
        Signature{c, r: (a - c*key), pk: *location}
    }

    /// verifies a schoore signature on a message given as a Sha3_512 state
    pub fn verify(&self, message: &mut Sha3_512, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>) -> bool { // the inputs are the hashed messages you are checking for signatures on because it's faster for many messages.
        if self.pk as usize >= stkstate.len() {return false}
        message.update((self.r*PEDERSEN_H() + self.c*stkstate.vec[self.pk as usize].0.decompress().unwrap()).compress().to_bytes());
        self.c == Scalar::from_hash(message.to_owned())
    }

    /// verifies a schoore signature on a message given as a Sha3_512 state
    /// this is for users because it requires less of stkstate
    pub fn verify_user(&self, message: &mut Sha3_512, stkstate: &Vec<(CompressedRistretto,u64)>) -> bool { // the inputs are the hashed messages you are checking for signatures on because it's faster for many messages.
        if self.pk as usize >= stkstate.len() {return false}
        message.update((self.r*PEDERSEN_H() + self.c*stkstate[self.pk as usize].0.decompress().unwrap()).compress().to_bytes());
        self.c == Scalar::from_hash(message.to_owned())
    }

    /// signs any message with your staker public key (without a location)
    pub fn sign_message2(key: &Scalar, message: &Vec<u8>) -> Vec<u8> {
        let mut s = Sha3_512::new();
        s.update(&message); // impliment non block check stuff for signatures
        let mut csprng = thread_rng();
        let a = Scalar::random(&mut csprng);
        s.update((a*PEDERSEN_H()).compress().to_bytes());
        let c = Scalar::from_hash(s.to_owned());
        let mut out = c.as_bytes().to_vec();
        out.par_extend((a - c*key).as_bytes());
        out.par_extend((key*PEDERSEN_H()).compress().as_bytes());
        out.par_extend(message);
        out
    }

    /// recieves a signed message and returns the key that signed it (for if it's not signed with a location)
    pub fn recieve_signed_message2(signed_message: &mut Vec<u8>) -> Option<CompressedRistretto> {
        if signed_message.len() >= 96 {
            let sig = signed_message.par_drain(..96).collect::<Vec<_>>();
            let c = Scalar::from_bits(sig[..32].try_into().unwrap());
            let r = Scalar::from_bits(sig[32..64].try_into().unwrap());
            let pk = CompressedRistretto::from_slice(&sig[64..96]);
            
            if let Some(pkdecom) = pk.decompress() {
    
                let mut h = Sha3_512::new();
                h.update(signed_message);
                h.update((r*PEDERSEN_H() + c*pkdecom).compress().to_bytes());
                
                if c == Scalar::from_hash(h) {
                    return Some(pk)
                }
            }
        }
        None
    }

    /// signs a message with the block number as a timestamp with your staker public key
    pub fn sign_message_nonced(key: &Scalar, message: &Vec<u8>, location: &usize, bnum: &u64) -> Vec<u8> {
        let mut s = Sha3_512::new();
        s.update(&message); // impliment non block check stuff for signatures
        s.update(bnum.to_le_bytes());
        let mut csprng = thread_rng();
        let a = Scalar::random(&mut csprng);
        s.update((a*PEDERSEN_H()).compress().to_bytes());
        let c = Scalar::from_hash(s.to_owned());
        let mut out = c.as_bytes().to_vec();
        out.par_extend((a - c*key).as_bytes());
        out.par_extend(location.to_le_bytes());
        out.par_extend(message);
        out
    }

    /// recieved a signed message only if it was signed as the current block
    pub fn recieve_signed_message_nonced(signed_message: &mut Vec<u8>, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>, bnum: &u64) -> Option<usize> {
        if signed_message.len() < 72 {return None}
        let sig = signed_message.par_drain(..72).collect::<Vec<_>>();
        let s = Signature{
            c: Scalar::from_bits(sig[..32].try_into().unwrap()),
            r: Scalar::from_bits(sig[32..64].try_into().unwrap()),
            pk: u64::from_le_bytes(sig[64..72].try_into().unwrap()) as usize
        };
        
        let mut h = Sha3_512::new();
        h.update(signed_message);
        h.update(bnum.to_le_bytes());
        if s.verify(&mut h, stkstate) {
            Some(s.pk)
        } else {
            None
        }
    }



}



#[derive(Default, Clone, Eq, Serialize, Deserialize, Hash, Debug)]
/// a full block that no one truly has to interact with except the comittee
pub struct NextBlock {
    pub validators: Vec<Signature>,
    pub leader: Signature,
    pub txs: Vec<PolynomialTransaction>,
    pub last_name: Vec<u8>,
    pub shard: u8,
    pub bnum: u64,
}
impl PartialEq for NextBlock {
    fn eq(&self, other: &Self) -> bool {
        bincode::serialize(self).unwrap() == bincode::serialize(other).unwrap()
    }
}
impl NextBlock {
    /// selects the transactions that are valid (as a member of the comittee in block generation)
    pub fn valicreate(key: &Scalar, location: &usize, leader: &CompressedRistretto, mut txs: Vec<PolynomialTransaction>, pool: &u8, bnum: &u64, last_name: &Vec<u8>, bloom: &BloomFile, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>, nonanony: &VecHashMap<CompressedRistretto,u64>) -> NextBlock {
        let mut stks = txs.par_iter().filter(|x| 
            if x.inputs.last() == Some(&1) {
                x.verifystk(&stkstate,bnum/NONCEYNESS).is_ok()
            } else {
                false
            }
        ).cloned().collect::<Vec<PolynomialTransaction>>(); /* i would use drain_filter but its unstable */
        stks = stks.par_iter().enumerate().filter(|(i,x)| {
            x.inputs.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap())).collect::<Vec<_>>().par_iter().all(|&x|
                !stks[..*i].par_iter().flat_map(|x| x.inputs.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap())).collect::<Vec<_>>()).collect::<Vec<u64>>()
                .contains(&x)
            )
        }).map(|x| x.1.to_owned()).collect::<Vec<_>>();
        let mut nonanons = txs.par_iter().filter(|x| 
            if x.inputs.last() == Some(&2) {
                x.verifynonanony(&nonanony,bnum/NONCEYNESS).is_ok()
            } else {
                false
            }
        ).cloned().collect::<Vec<PolynomialTransaction>>(); /* i would use drain_filter but its unstable */
        nonanons = nonanons.par_iter().enumerate().filter(|(i,x)| {
            x.inputs.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap())).collect::<Vec<_>>().par_iter().all(|&x|
                !nonanons[..*i].par_iter().flat_map(|x| x.inputs.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap())).collect::<Vec<_>>()).collect::<Vec<u64>>()
                .contains(&x)
            )
        }).map(|x| x.1.to_owned()).collect::<Vec<_>>();
        txs.retain(|x| x.inputs.last() == Some(&0));
        
        
        
        txs =
            txs.par_iter().enumerate().filter_map(|(i,x)| {
                if 
                x.tags.par_iter().all(|&x| !txs.iter().take(i).flat_map(|x| x.tags.clone()).collect::<HashSet<CompressedRistretto>>().contains(&x))
                &&
                x.tags.iter().all(|y| {!bloom.contains(&y.to_bytes())})
                &&
                x.tags.len() == x.tags.iter().collect::<HashSet<_>>().len()
                &&
                x.verify().is_ok()
                {
                    Some(x.to_owned())
                }
                else {None}
        }).collect::<Vec<PolynomialTransaction>>();
        txs.append(&mut stks);
        txs.append(&mut nonanons);


        let m = vec![leader.to_bytes().to_vec(),vec![*pool],Syncedtx::to_sign(&txs,nonanony,stkstate),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);
        NextBlock {
            validators: vec![],
            leader: Signature::sign(&key,&mut s,&location),
            txs: txs.to_owned(),
            last_name: last_name.to_owned(),
            shard: *pool,
            bnum: *bnum,
        }
    }

    /// creates a full block from a collection of signatures in the comittee
    pub fn finish(key: &Scalar, location: &usize, sigs: &Vec<NextBlock>, validator_pool: &Vec<usize>, pool: &u8, bnum: &u64, last_name: &Vec<u8>, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>, nonanony: &VecHashMap<CompressedRistretto,u64>) -> Result<NextBlock,&'static str> { // <----do i need to reference previous block explicitly?
        let leader = (key*PEDERSEN_H()).compress().as_bytes().to_vec();
        let mut sigs = sigs.into_par_iter().filter(|x| !validator_pool.into_par_iter().all(|y| x.leader.pk != *y)).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
        let mut sigfinale: Vec<NextBlock>;
        for _ in 0..=(sigs.len() - SIGNING_CUTOFF) {
            let b = sigs.pop().unwrap();
            sigfinale = sigs.par_iter().filter(|x| if let (Ok(z),Ok(y)) = (bincode::serialize(&x.txs),bincode::serialize(&b.txs)) {z==y} else {false}).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
            if validator_pool.par_iter().filter(|x| !sigfinale.par_iter().all(|y| x.to_owned() != &y.leader.pk)).count() >= SIGNING_CUTOFF {
                sigfinale.push(b);
                // println!("they agree on tx in block validation");
                let sigfinale = sigfinale.par_iter().enumerate().filter_map(|(i,x)| if sigs[..i].par_iter().all(|y| x.leader.pk != y.leader.pk) {Some(x.to_owned())} else {None}).collect::<Vec<NextBlock>>();
                let m = vec![leader.clone(),vec![*pool],Syncedtx::to_sign(&sigfinale[0].txs,nonanony,stkstate),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
                let mut s = Sha3_512::new();
                s.update(&m);
                let sigfinale = sigfinale.into_par_iter().filter(|x| Signature::verify(&x.leader, &mut s.clone(),&stkstate)).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
                let signers = sigfinale.clone();
                let sigfinale = sigfinale.into_par_iter().enumerate().filter_map(|(e,x)|
                    if signers[..e].par_iter().all(|y| y.leader.pk != x.leader.pk) {
                        Some(x)
                    } else {
                        None
                    }
                ).collect::<Vec<_>>();
                let sigs = sigfinale.par_iter().map(|x| x.leader.to_owned()).collect::<Vec<Signature>>();
                let mut s = Sha3_512::new();
                s.update(&bincode::serialize(&sigs).unwrap().to_vec());
                let c = s.finalize();
                let m = vec![vec![*pool],bnum.to_le_bytes().to_vec(),last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
                let mut s = Sha3_512::new();
                s.update(&m);
                let leader = Signature::sign(&key, &mut s,&location);
                if validator_pool.par_iter().filter(|x| !sigfinale.par_iter().all(|y| x.to_owned() != &y.leader.pk)).count() > SIGNING_CUTOFF {
                    return Ok(NextBlock{validators: sigs, leader, txs: sigfinale[0].txs.to_owned(), last_name: last_name.to_owned(), shard: *pool, bnum: bnum.to_owned()})
                } else {
                    print!("not enough sigs... ");
                    break
                }
            }
        }
        println!("failed to make block :(");
        return Err("Not enoguh true sigs")
    }

    /// verifies a full block (that the comittee acted as they should)
    pub fn verify(&self, validator_pool: &Vec<usize>, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>, nonanony:&VecHashMap<CompressedRistretto, u64>) -> Result<bool, &'static str> {
        let mut s = Sha3_512::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![vec![self.shard],self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);

        if !self.leader.verify(&mut s, &stkstate) {
            return Err("leader is fake")
        }
        let m = vec![stkstate.vec[self.leader.pk as usize].0.as_bytes().to_vec().clone(),vec![self.shard],Syncedtx::to_sign(&self.txs,nonanony,stkstate),self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut h = Sha3_512::new();
        h.update(&m);
        if !self.validators.par_iter().all(|x| x.verify(&mut h.clone(), &stkstate)) {
            return Err("at least 1 validator is fake")
        }
        if !self.validators.par_iter().all(|x| !validator_pool.par_iter().all(|y| x.pk != *y)) {
            return Err("at least 1 validator is not in the pool")
        }
        if validator_pool.par_iter().filter(|x| !self.validators.par_iter().all(|y| x.to_owned() != &y.pk)).count() <= SIGNING_CUTOFF {
            return Err("there aren't enough validators")
        }
        let x = self.validators.par_iter().map(|x| x.pk).collect::<Vec<_>>();
        if !x.clone().par_iter().enumerate().all(|(i,y)| x[..i].par_iter().all(|z| y != z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }

    /// the function you use to pay the comittee if you are not saving the block
    pub fn pay_all_empty(shard: &usize, comittee: &Vec<Vec<usize>>, valinfo: &mut VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>, reward: f64) {
        let winners = comittee_n(*shard, comittee, valinfo);
        let inflation = (reward/winners.len() as f64) as u64;
        for i in winners {
            valinfo.vec[i].1.0 += inflation;
        }
    }

    /// the function you use to pay the comittee if you are not saving the block
    /// this is for users because it does not require the inverse hashmap
    pub fn pay_all_empty_user(shard: &usize, comittee: &Vec<Vec<usize>>, valinfo: &mut Vec<(CompressedRistretto,u64)>, reward: f64) {
        let winners = comittee_n_user(*shard, comittee, valinfo);
        let inflation = (reward/winners.len() as f64) as u64;
        for i in winners {
            valinfo[i].1 += inflation;
        }
    }

    /// the function you use to pay yourself if you are not saving the block
    pub fn pay_self_empty(shard: &usize, comittee: &Vec<Vec<usize>>, mine: &mut Option<(usize,u64)>, reward: f64) -> bool {

        let mut changed = false;
        if let Some(mine) = mine {
            let winners = comittee[*shard].iter();
            let inflation = (reward/winners.len() as f64) as u64;
            for &i in winners {
                if mine.0 == i {
                    changed = true;
                    mine.1 += inflation;
                }
            }
        }
        changed
    }

    /// reads the specified block from the file
    pub fn read(bnum: &u64) -> Result<Vec<u8>,&'static str> {
        let mut r = BufReader::new(File::open("fullblocks_metadata").unwrap());
        let mut datalocation = [0u8;16];
        r.seek(SeekFrom::Start(bnum*8)).expect("Seek failed");
        let bytes_read = r.read(&mut datalocation).expect("should work");
        if bytes_read == 16 {
            let loc1 = u64::from_le_bytes(datalocation[..8].try_into().unwrap());
            let loc2 = u64::from_le_bytes(datalocation[8..].try_into().unwrap());
            let mut bytes = (loc1..loc2).map(|_| 0u8).collect::<Vec<_>>();
            let mut r = BufReader::new(File::open("fullblocks").unwrap());
            r.seek(SeekFrom::Start(loc1)).expect("Seek failed");
            r.read(&mut bytes).unwrap();
            if bytes.is_empty() {
                Err("We skipped that block")
            } else {
                Ok(bytes)
            }
        } else if bytes_read == 8 {
            let loc1 = u64::from_le_bytes(datalocation[..8].try_into().unwrap());
            let mut bytes = vec![];
            let mut r = BufReader::new(File::open("fullblocks").unwrap());
            r.seek(SeekFrom::Start(loc1)).expect("Seek failed");
            r.read_to_end(&mut bytes).unwrap();
            if bytes.is_empty() {
                Err("We skipped this block")
            } else {
                Ok(bytes)
            }
        } else {
            Err("We don't have that block")
        }
    }

    /// saves the block to the full block file and adds the metadata to the metadata file
    /// you must save empty blocks to metadata for this to work (save vec![])
    pub fn save(serialized_block: &Vec<u8>) {
        let mut f = OpenOptions::new().append(true).open("fullblocks").unwrap();
        f.write(&serialized_block).expect("should work");
        let currentlen = f.seek(SeekFrom::End(0)).expect("Seek failed");
        let mut f = OpenOptions::new().append(true).open("fullblocks_metadata").unwrap();
        f.write(&currentlen.to_le_bytes()).expect("should work");
    }

    /// created the full block file and metadata to the metadata file
    pub fn initialize_saving() {
        File::create("fullblocks").expect("should work");
        let mut f = File::create("fullblocks_metadata").expect("should work");
        f.write(&[0u8;8]).expect("should work");
    }

    /// converts a full block into lightning block
    pub fn tolightning(&self,nonanony: &VecHashMap<CompressedRistretto, u64>, stkinfo: &VecHashMap<CompressedRistretto, (u64, Option<SocketAddr>)>) -> LightningSyncBlock {
        LightningSyncBlock {
            validators: self.validators.to_owned(),
            leader: self.leader.to_owned(),
            info: Syncedtx::from(&self.txs,nonanony,stkinfo),
            shard: self.shard,
            bnum: self.bnum.to_owned(),
            last_name: self.last_name.to_owned(),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Hash, Debug)]
/// a shrunk block in the blockchain with less structure and not containing any proofs that would be in the full block
pub struct LightningSyncBlock {
    pub validators: Vec<Signature>,
    pub leader: Signature,
    pub info: Syncedtx,
    pub shard: u8,
    pub bnum: u64,
    pub last_name: Vec<u8>,
}
impl LightningSyncBlock {
    /// verifies that the block is real and the 128 comittee members came to consensus. all computations are carried out in parallell
    pub fn verify_multithread(&self, validator_pool: &Vec<usize>, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>) -> Result<bool, &'static str> {
        let mut s = Sha3_512::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![vec![self.shard],self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut h = Sha3_512::new();
        h.update(&m);
        if !self.leader.verify(&mut h, &stkstate) {
            return Err("leader is fake")
        }
        let m = vec![stkstate.vec[self.leader.pk as usize].0.as_bytes().to_vec().clone(), vec![self.shard], bincode::serialize(&self.info).unwrap(), self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut h = Sha3_512::new();
        h.update(&m);
        if !self.validators.par_iter().all(|x| x.verify(&mut h.clone(), &stkstate)) {
            return Err("at least 1 validator is fake")
        }
        if !self.validators.par_iter().any(|x| validator_pool.into_par_iter().any(|y| x.pk == *y)) {
            return Err("at least 1 validator is not in the pool")
        }
        if validator_pool.par_iter().filter(|x| self.validators.par_iter().any(|y| x.to_owned() == &y.pk)).count() < SIGNING_CUTOFF {
            return Err("there aren't enough validators")
        }
        let x = self.validators.par_iter().map(|x| x.pk).collect::<Vec<_>>();
        if x.par_iter().enumerate().any(|(i,y)| x.par_iter().take(i).any(|z| y == z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }

    /// verifies that the block is real and the 128 comittee members came to consensus. all computations are carried out in parallell
    /// this is for users because it requires less of stkstate
    pub fn verify_multithread_user(&self, validator_pool: &Vec<usize>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Result<bool, &'static str> {
        let mut s = Sha3_512::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![vec![self.shard],self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut h = Sha3_512::new();
        h.update(&m);
        if !self.leader.verify_user(&mut h, &stkstate) {
            return Err("leader is fake")
        }
        let m = vec![stkstate[self.leader.pk as usize].0.as_bytes().to_vec().clone(), vec![self.shard], bincode::serialize(&self.info).unwrap(), self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut h = Sha3_512::new();
        h.update(&m);
        if !self.validators.par_iter().all(|x| x.verify_user(&mut h.clone(), &stkstate)) {
            return Err("at least 1 validator is fake")
        }
        if !self.validators.par_iter().any(|x| validator_pool.into_par_iter().any(|y| x.pk == *y)) {
            return Err("at least 1 validator is not in the pool")
        }
        if validator_pool.par_iter().filter(|x| self.validators.par_iter().any(|y| x.to_owned() == &y.pk)).count() < SIGNING_CUTOFF {
            return Err("there aren't enough validators")
        }
        let x = self.validators.par_iter().map(|x| x.pk).collect::<Vec<_>>();
        if x.par_iter().enumerate().any(|(i,y)| x.par_iter().take(i).any(|z| y == z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }

    /// verifies that the block is real and the 128 comittee members came to consensus
    pub fn verify(&self, validator_pool: &Vec<usize>, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>) -> Result<bool, &'static str> {
        let mut s = Sha3_512::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![vec![self.shard],self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut h = Sha3_512::new();
        h.update(&m);
        if !self.leader.verify(&mut h, &stkstate) {
            return Err("leader is fake")
        }
        let m = vec![stkstate.vec[self.leader.pk as usize].0.as_bytes().to_vec().clone(), vec![self.shard], bincode::serialize(&self.info).unwrap(), self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut h = Sha3_512::new();
        h.update(&m);
        if !self.validators.iter().all(|x| x.verify(&mut h.clone(), &stkstate)) {
            return Err("at least 1 validator is fake")
        }
        if !self.validators.iter().all(|x| validator_pool.into_iter().any(|y| x.pk == *y)) {
            return Err("at least 1 validator is not in the pool")
        }
        if validator_pool.iter().filter(|x| !self.validators.iter().all(|y| x.to_owned() != &y.pk)).count() < SIGNING_CUTOFF {
            return Err("there aren't enough validators")
        }
        let x = self.validators.iter().map(|x| x.pk).collect::<Vec<_>>();
        if x.iter().enumerate().any(|(i,y)| x.iter().take(i).any(|z| y == z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }

    /// updates the staker state by dulling out punishments and gifting rewards. also updates the queue, exitqueue, and comittee if stakers left
    pub fn scan_as_noone(&self, valinfo: &mut VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>, nonanony: &mut VecHashMap<CompressedRistretto,u64>, queue: &mut Vec<VecDeque<usize>>, exitqueue: &mut Vec<VecDeque<usize>>, comittee: &mut Vec<Vec<usize>>, reward: f64, save_history: bool) {
        if save_history {History::append(&self.info.txout)};

        queue.par_iter_mut().for_each(|y| {
            *y = y.into_par_iter().filter_map(|z| {
                follow(*z,&self.info.stkout,valinfo.len())
            }).collect::<VecDeque<_>>();
        });
        comittee.par_iter_mut().zip(exitqueue.par_iter_mut()).for_each(|(y,z)| {
            let a = y.into_par_iter().map(|y| {
                follow(*y,&self.info.stkout,valinfo.len())
            }).collect::<Vec<_>>();
            let b = a.par_iter().enumerate().filter(|(_,x)| x.is_none()).map(|(x,_)|x).collect::<Vec<_>>();
            *z = z.par_iter().filter_map(|x| {
                if b.contains(x) {
                    None
                } else {
                    Some(*x - b.par_iter().filter(|y| *y<x).count())
                }
            }).collect();
            *y = a.into_par_iter().filter_map(|x|x).collect();
        });
        refill(queue, exitqueue, comittee, valinfo);


        // println!("valinfo {:?}",valinfo);



        self.info.stkin.iter().for_each(|x| {
            valinfo.mut_value_from_index(x.0).0 = x.1;
        });
        let winners: Vec<usize>;
        let masochists: Vec<usize>;
        let x = self.validators.iter().map(|x| x.pk as usize).collect::<HashSet<_>>();

        let cn = comittee_n(self.shard as usize, comittee, valinfo);
        let lucky = comittee_n(self.shard as usize+1, comittee, valinfo);
        winners = cn.iter().filter(|&y| x.contains(y)).map(|x| *x).collect::<Vec<_>>();
        masochists = cn.iter().filter(|&y| !x.contains(y)).map(|x| *x).collect::<Vec<_>>();
        let fees = self.info.fees/(winners.len() as u64);
        let inflation = (reward/winners.len() as f64) as u64;

        for i in winners {
            valinfo.vec[i].1.0 += inflation;
            valinfo.vec[i].1.0 += fees;
        }
        let mut punishments = 0u64;
        for i in masochists {
            punishments += valinfo.vec[i].1.0/PUNISHMENT_FRACTION;
            valinfo.vec[i].1.0 -= valinfo.vec[i].1.0/PUNISHMENT_FRACTION;
        }
        punishments = punishments/lucky.len() as u64;
        for i in lucky {
            valinfo.vec[i].1.0 += punishments;
        }







        valinfo.remove_all(&self.info.stkout);
        valinfo.insert_all(&self.info.stknew.iter().map(|x| (x.0,(x.1,None))).collect::<Vec<_>>());






        self.info.nonanonyin.iter().for_each(|x| {
            let y = nonanony.mut_value_from_index(x.0);
            *y = x.1;
        });
        nonanony.remove_all(&self.info.nonanonyout);
        nonanony.insert_all(&self.info.nonanonynew);
    


        // println!("nonanonyinfo {:?}",nonanony.vec);
        // println!("valinfo {:?}",valinfo.vec);
        // println!("valinfo {:?}",self.info.stkin);

    }

    /// updates the staker state by dulling out punishments and gifting rewards. also updates the queue, exitqueue, and comittee if stakers left
    /// this is for users because it requires less of valinfo
    pub fn scan_as_noone_user(&self, valinfo: &mut Vec<(CompressedRistretto,u64)>, queue: &mut Vec<VecDeque<usize>>, exitqueue: &mut Vec<VecDeque<usize>>, comittee: &mut Vec<Vec<usize>>, reward: f64, save_history: bool) {
        if save_history {History::append(&self.info.txout)};

        queue.par_iter_mut().for_each(|y| {
            *y = y.into_par_iter().filter_map(|z| {
                follow(*z,&self.info.stkout,valinfo.len())
            }).collect::<VecDeque<_>>();
        });
        comittee.par_iter_mut().zip(exitqueue.par_iter_mut()).for_each(|(y,z)| {
            let a = y.into_par_iter().map(|y| {
                follow(*y,&self.info.stkout,valinfo.len())
            }).collect::<Vec<_>>();
            let b = a.par_iter().enumerate().filter(|(_,x)| x.is_none()).map(|(x,_)|x).collect::<Vec<_>>();
            *z = z.par_iter().filter_map(|x| {
                if b.contains(x) {
                    None
                } else {
                    Some(*x - b.par_iter().filter(|y| *y<x).count())
                }
            }).collect();
            *y = a.into_par_iter().filter_map(|x|x).collect();
        });
        refill_user(queue, exitqueue, comittee, valinfo);


        // println!("valinfo {:?}",valinfo);

        self.info.stkin.iter().for_each(|x| {
            valinfo[x.0].1 = x.1;
        });


        let winners: Vec<usize>;
        let masochists: Vec<usize>;
        let x = self.validators.iter().map(|x| x.pk as usize).collect::<HashSet<_>>();

        let cn = comittee_n_user(self.shard as usize, comittee, valinfo);
        let lucky = comittee_n_user(self.shard as usize+1, comittee, valinfo);
        winners = cn.iter().filter(|&y| x.contains(y)).map(|x| *x).collect::<Vec<_>>();
        masochists = cn.iter().filter(|&y| !x.contains(y)).map(|x| *x).collect::<Vec<_>>();
        let fees = self.info.fees/(winners.len() as u64);
        let inflation = (reward/winners.len() as f64) as u64;


        for i in winners {
            valinfo[i].1 += inflation;
            valinfo[i].1 += fees;
        }
        let mut punishments = 0u64;
        for i in masochists {
            punishments += valinfo[i].1/PUNISHMENT_FRACTION;
            valinfo[i].1 -= valinfo[i].1/PUNISHMENT_FRACTION;
        }
        punishments = punishments/lucky.len() as u64;
        for i in lucky {
            valinfo[i].1 += punishments;
        }







        valinfo.remove_all(&self.info.stkout);
        valinfo.par_extend(&self.info.stknew);

    }



    /// scans the block for money sent to you. additionally updates your understanding of the height. returns weather you recieved money
    pub fn scan(&self, me: &Account, mine: &mut HashMap<u64,OTAccount>, reversemine: &mut HashMap<CompressedRistretto,u64>, height: &mut u64, alltagsever: &mut HashSet<CompressedRistretto>) -> bool {
        let newmine = self.info.txout.par_iter().enumerate().filter_map(|(i,x)| if let Ok(y) = me.receive_ot(x) {Some((i as u64+*height,y))} else {None}).collect::<Vec<(u64,OTAccount)>>();
        let mut imtrue = newmine.is_empty();
        for (n,m) in newmine {
            if alltagsever.contains(&m.tag.unwrap()) {
                println!("someone sent you money that you can't spend");
            } else {
                // let t = std::time::Instant::now();
                alltagsever.insert(m.tag.unwrap());
                // println!("insert tag: {}",t.elapsed().as_millis());
                // let t = std::time::Instant::now();
                reversemine.insert(m.tag.unwrap(),n);
                mine.insert(n,m);
                imtrue = true;
                // println!("recieve money: {}",t.elapsed().as_millis());
            }
        }
        for tag in self.info.tags.iter() {
            if let Some(loc) = reversemine.get(tag) {
                mine.remove(loc);
                reversemine.remove(tag);
            }
        }

        *height += self.info.txout.len() as u64;

        imtrue
    }

    /// scans the block for any transactions sent to you or any rewards and punishments you recieved. it additionally updates the height of stakers. returns if your money changed then if your index changed
    pub fn scanstk(&self, me: &Account, mine: &mut Option<(usize,u64)>, maybesome: bool, height: &mut usize, comittee: &Vec<Vec<usize>>, reward: f64, valinfo: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>) -> (bool,bool) {

        // let t = std::time::Instant::now();

        // println!("mine:---------------------------------\n{:?}",mine);
        let changed = mine.clone();
        let nonetosome = mine.is_none();
        if maybesome {
            // println!("mine: {:?}",mine);
            if let Some(m) = mine {
                if let Some(x) = self.info.stkin.par_iter().find_first(|x| x.0 == m.0) {
                    m.1 = x.1;
                    // println!("=================================================================================================");
                    // println!("someone sent me more stake! at staking {:?}",m);
                    // println!("=================================================================================================");
                }
            }
    
            if let Some(x) = &mine {
                *mine = follow(x.0,&self.info.stkout,*height).map(|y| (y,x.1));
            }
            *height -= self.info.stkout.len();
    
            let cr = me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
            if let Some(x) = self.info.stknew.par_iter().enumerate().find_first(|x| x.1.0 == cr) {
                *mine = Some((*height + x.0,x.1.1));
                // println!("=================================================================================================");
                // println!("someone sent me new stake! at staking {:?}",mine);
                // println!("=================================================================================================");
            }
            *height += self.info.stknew.len();
        } else {
            *height -= self.info.stkout.len();
            *height += self.info.stkin.len();
        }
        if let Some(mine) = mine {
            let winners: Vec<usize>;
            let masochists: Vec<usize>;
    
            let x = self.validators.iter().map(|x| x.pk as usize).collect::<HashSet<_>>();
    
            let cn = comittee_n(self.shard as usize, comittee, valinfo);
            let lucky = comittee_n(self.shard as usize+1, comittee, valinfo);
            winners = cn.iter().filter(|&y| x.contains(y)).map(|x| *x).collect::<Vec<_>>();
            masochists = cn.iter().filter(|&y| !x.contains(y)).map(|x| *x).collect::<Vec<_>>();
            
            let fees = self.info.fees/(winners.len() as u64);
            let inflation = (reward/winners.len() as f64) as u64;
    
            for i in winners {
                if mine.0 == i {
                    mine.1 += inflation;
                    mine.1 += fees;
                };
            }
            let mut punishments = 0u64;
            for i in masochists {
                punishments += valinfo.vec[i].1.0/PUNISHMENT_FRACTION;
                if mine.0 == i {
                    mine.1 -= valinfo.vec[i].1.0/PUNISHMENT_FRACTION;
                }
            }
            punishments = punishments/lucky.len() as u64;
            for i in lucky {
                if mine.0 == i {
                    mine.1 += punishments;
                }
            }
        }



        // println!("mine:---------------------------------\n{:?}",mine);
        (changed != *mine, nonetosome && mine.is_some())


    }

    /// scans the block for any transactions sent to or from you. it additionally updates the height of nonanonys. returns if your money changed
    pub fn scannonanony(&self, me: &Account, mine: &mut Option<(usize,u64)>, height: &mut usize) -> bool {

        // let t = std::time::Instant::now();

        let changed = mine.clone();
        if let Some(m) = mine {
            if let Some(x) = self.info.nonanonyin.par_iter().find_first(|x| x.0 == m.0) {
                m.1 = x.1;
                // println!("=================================================================================================");
                // println!("someone sent me more money! at {:?}",m);
                // println!("=================================================================================================");
            }
        }
        *height -= self.info.nonanonyout.len();

        if let Some(x) = &mine {
            *mine = follow(x.0,&self.info.nonanonyout,*height).map(|y| (y,x.1));
        }

        let cr = me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
        if let Some(x) = self.info.nonanonynew.par_iter().enumerate().find_first(|x| x.1.0 == cr) {
            // println!("=================================================================================================");
            // println!("someone sent me new money! at {:?}",(*height + x.0,x.1.1));
            // println!("=================================================================================================");

            *mine = Some((*height + x.0,x.1.1));
        }
        *height += self.info.nonanonynew.len();




        // if self.bnum%10 == 0 {
            // println!("ideal: {:?}",me.nonanony_acc().derive_stk_ot(&Scalar::one()).pk.compress());
            // println!("in: {:?}",self.info.nonanonyin);
            // println!("grow: {:?}",self.info.nonanonynew);
            // println!("staker reference: {:?}",self.info.stkin);
            // println!("random: {:?}",self.info.txout.iter().map(|x| x.pk.compress()).collect::<Vec<_>>());
        // }


        // println!("nminen {:?}",mine);
        // println!("heightn {}",height);
        
        // println!("{}",t.elapsed().as_millis());

        changed != *mine


    }

    /// reads the specified block from the file
    pub fn read(bnum: &u64) -> Result<Vec<u8>,&'static str> {
        let mut r = BufReader::new(File::open("lightningblocks_metadata").unwrap());
        let mut datalocation = [0u8;16];
        r.seek(SeekFrom::Start(bnum*8)).expect("Seek failed");
        let bytes_read = r.read(&mut datalocation).expect("should work");
        if bytes_read == 16 {
            let loc1 = u64::from_le_bytes(datalocation[..8].try_into().unwrap());
            let loc2 = u64::from_le_bytes(datalocation[8..].try_into().unwrap());
            let mut bytes = (loc1..loc2).map(|_| 0u8).collect::<Vec<_>>();
            let mut r = BufReader::new(File::open("lightningblocks").unwrap());
            r.seek(SeekFrom::Start(loc1)).expect("Seek failed");
            r.read(&mut bytes).unwrap();
            if bytes.is_empty() {
                Err("We skipped that block")
            } else {
                Ok(bytes)
            }
        } else if bytes_read == 8 {
            let loc1 = u64::from_le_bytes(datalocation[..8].try_into().unwrap());
            let mut bytes = vec![];
            let mut r = BufReader::new(File::open("lightningblocks").unwrap());
            r.seek(SeekFrom::Start(loc1)).expect("Seek failed");
            r.read_to_end(&mut bytes).unwrap();
            if bytes.is_empty() {
                Err("We skipped this block")
            } else {
                Ok(bytes)
            }
        } else {
            Err("We don't have that block")
        }
    }

    /// saves the block to the full block file and adds the metadata to the metadata file
    /// you must save empty blocks to metadata for this to work (save vec![])
    pub fn save(serialized_block: &Vec<u8>) {
        let mut f = OpenOptions::new().append(true).open("lightningblocks").unwrap();
        f.write(&serialized_block).expect("should work");
        let currentlen = f.seek(SeekFrom::End(0)).expect("Seek failed");
        let mut f = OpenOptions::new().append(true).open("lightningblocks_metadata").unwrap();
        f.write(&currentlen.to_le_bytes()).expect("should work");
    }

    /// created the full block file and metadata to the metadata file
    pub fn initialize_saving() {
        File::create("lightningblocks").expect("should work");
        let mut f = File::create("lightningblocks_metadata").expect("should work");
        f.write(&[0u8;8]).expect("should work");
    }
    /// adds all tags to the bloom filter so validators can check for double spends
    pub fn update_bloom(&self,bloom:&BloomFile,parallel:bool) {
        if parallel {
            self.info.tags.par_iter().for_each(|x| bloom.insert(&x.as_bytes()));
        } else {
            self.info.tags.iter().for_each(|x| bloom.insert(&x.as_bytes()));
        }
    }




}


/// selects the stakers who get to validate the queue and exit_queue
pub fn select_stakers(block: &Vec<u8>, bnum: &u64, shard: &u128, queue: &mut VecDeque<usize>, exitqueue: &mut VecDeque<usize>, comittee: &mut Vec<usize>, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>) {
    let y = stkstate.vec.par_iter().map(|(_,y)| y.0 as u128).collect::<Vec<_>>();
    let tot_stk: u128 = y.par_iter().sum(); /* initial queue will be 0 for all non0 shards... */

    let bnum = bnum.to_le_bytes();
    let mut s = AHasher::new_with_keys(0, *shard);
    s.write(&block);
    s.write(&bnum);
    let mut winner = (0..REPLACERATE).into_par_iter().map(|x| {
        let mut s = s.clone();
        s.write(&x.to_le_bytes()[..]);
        let c = s.finish() as u128;
        // println!("unmoded winner:    {}",c);
        let mut staker = (c%tot_stk) as i128;
        // println!("winner:            {}",staker);
        let mut w = 0;
        for (i,&j) in y.iter().enumerate() {
            staker -= j as i128;
            if staker <= 0
                {w = i; break;}
        };
        w
    }).collect::<VecDeque<usize>>();
    queue.append(&mut winner); // need to hardcode initial state
    let winner = queue.par_drain(..REPLACERATE).collect::<Vec<usize>>();

    let mut s = AHasher::new_with_keys(1, *shard);
    s.write(&block);
    s.write(&bnum);
    let mut loser = (0..REPLACERATE).into_par_iter().map(|x| {
        let mut s = s.clone();
        s.write(&x.to_le_bytes()[..]);
        let c = s.finish() as usize;
        c%NUMBER_OF_VALIDATORS
    }).collect::<VecDeque<usize>>();
    exitqueue.append(&mut loser);
    let loser = exitqueue.par_drain(..REPLACERATE).collect::<Vec<usize>>();

    for (i,j) in loser.iter().enumerate() {
        comittee[*j] = winner[i];
    }
}

pub fn refill(queue: &mut Vec<VecDeque<usize>>, exitqueue: &mut Vec<VecDeque<usize>>, comittee: &mut Vec<Vec<usize>>, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>) {

    let y = stkstate.vec.par_iter().map(|(_,(y,_))| *y as u128).collect::<Vec<_>>();
    let tot_stk: u128 = y.par_iter().sum(); /* initial queue will be 0 for all non0 shards... */

    queue.par_iter_mut().enumerate().for_each(|(shard,x)| {
        if x.len() < QUEUE_LENGTH {
            let mut s = AHasher::new_with_keys(shard as u128, shard as u128);
            s.write(b"refill_queue");
            s.write(&x.iter().map(|x| x.to_le_bytes()).flatten().collect::<Vec<_>>());
            if x.len() == 0 {
                *x = (0..QUEUE_LENGTH).map(|x| {
                    let mut s = s.clone();
                    s.write(&x.to_le_bytes()[..]);
                    let c = s.finish() as u128;

                    let mut staker = (c%tot_stk) as i128;
                    let mut w = 0;
                    for (i,&j) in y.iter().enumerate() {
                        staker -= j as i128;
                        if staker <= 0
                            {w = i; break;}
                    };
                    w
                }).collect::<VecDeque<_>>();
            } else {
                x.par_extend((0..QUEUE_LENGTH-x.len()).map(|y| {
                    let mut s = s.clone();
                    s.write(&y.to_le_bytes()[..]);
                    let c = s.finish() as usize;

                    x[c%x.len()]
                }).collect::<Vec<_>>());
            }
        }
    });
    exitqueue.par_iter_mut().enumerate().for_each(|(shard,x)| {
        if x.len() < QUEUE_LENGTH {
            let mut s = AHasher::new_with_keys(shard as u128, shard as u128);
            s.write(b"refill_exitqueue");
            s.write(&x.iter().map(|x| x.to_le_bytes()).flatten().collect::<Vec<_>>());
            if x.len() == 0 {
                *x = (0..QUEUE_LENGTH).map(|x| {
                    let mut s = s.clone();
                    s.write(&x.to_le_bytes()[..]);
                    let c = s.finish() as u128;

                    let mut staker = (c%tot_stk) as i128;
                    let mut w = 0;
                    for (i,&j) in y.iter().enumerate() {
                        staker -= j as i128;
                        if staker <= 0
                            {w = i; break;}
                    };
                    w
                }).collect::<VecDeque<_>>();
            } else {
                x.par_extend((0..QUEUE_LENGTH-x.len()).map(|y| {
                    let mut s = s.clone();
                    s.write(&y.to_le_bytes()[..]);
                    let c = s.finish() as usize;

                    x[c%x.len()]
                }).collect::<Vec<_>>());
            }
        }
    });
    comittee.par_iter_mut().enumerate().for_each(|(shard,x)| {
        if x.len() < NUMBER_OF_VALIDATORS {
            let mut s = AHasher::new_with_keys(shard as u128, shard as u128);
            s.write(b"refill_comittee");
            s.write(&x.iter().map(|x| x.to_le_bytes()).flatten().collect::<Vec<_>>());
            if x.len() == 0 {
                *x = (0..NUMBER_OF_VALIDATORS).map(|x| {
                    let mut s = s.clone();
                    s.write(&x.to_le_bytes()[..]);
                    let c = s.finish() as u128;

                    let mut staker = (c%tot_stk) as i128;
                    let mut w = 0;
                    for (i,&j) in y.iter().enumerate() {
                        staker -= j as i128;
                        if staker <= 0
                            {w = i; break;}
                    };
                    w
                }).collect::<Vec<_>>();
            } else {
                x.par_extend((0..NUMBER_OF_VALIDATORS-x.len()).map(|y| {
                    let mut s = s.clone();
                    s.write(&y.to_le_bytes()[..]);
                    let c = s.finish() as usize;

                    x[c%x.len()]
                }).collect::<Vec<_>>());
            }
        }
    });

}



/// selects the stakers who get to validate the queue and exit_queue
pub fn select_stakers_user(block: &Vec<u8>, bnum: &u64, shard: &u128, queue: &mut VecDeque<usize>, exitqueue: &mut VecDeque<usize>, comittee: &mut Vec<usize>, stkstate: &Vec<(CompressedRistretto,u64)>) {
    let y = stkstate.iter().map(|(_,y)| *y as u128).collect::<Vec<_>>();
    let tot_stk: u128 = y.par_iter().sum(); /* initial queue will be 0 for all non0 shards... */

    let bnum = bnum.to_le_bytes();
    let mut s = AHasher::new_with_keys(0, *shard);
    s.write(&block);
    s.write(&bnum);
    let mut winner = (0..REPLACERATE).into_par_iter().map(|x| {
        let mut s = s.clone();
        s.write(&x.to_le_bytes()[..]);
        let c = s.finish() as u128;
        // println!("unmoded winner:    {}",c);
        let mut staker = (c%tot_stk) as i128;
        // println!("winner:            {}",staker);
        let mut w = 0;
        for (i,&j) in y.iter().enumerate() {
            staker -= j as i128;
            if staker <= 0
                {w = i; break;}
        };
        w
    }).collect::<VecDeque<usize>>();
    queue.append(&mut winner); // need to hardcode initial state
    let winner = queue.par_drain(..REPLACERATE).collect::<Vec<usize>>();

    let mut s = AHasher::new_with_keys(1, *shard);
    s.write(&block);
    s.write(&bnum);
    let mut loser = (0..REPLACERATE).into_par_iter().map(|x| {
        let mut s = s.clone();
        s.write(&x.to_le_bytes()[..]);
        let c = s.finish() as usize;
        c%NUMBER_OF_VALIDATORS
    }).collect::<VecDeque<usize>>();
    exitqueue.append(&mut loser);
    let loser = exitqueue.par_drain(..REPLACERATE).collect::<Vec<usize>>();

    for (i,j) in loser.iter().enumerate() {
        comittee[*j] = winner[i];
    }
}


pub fn set_comittee_n(n: usize, comittee: &Vec<Vec<usize>>, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>) -> Vec<Vec<usize>> {
    (0..LONG_TERM_SHARDS).map(|x| {
        let n = x + n;
        if n < comittee.len() {
            comittee[n].clone()
        } else {
            instant_comittee(n, stkstate)
        }
    }).collect::<Vec<_>>()
}
pub fn set_comittee_n_user(n: usize, comittee: &Vec<Vec<usize>>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Vec<Vec<usize>> {
    (0..LONG_TERM_SHARDS).map(|x| {
        let n = x + n;
        if n < comittee.len() {
            comittee[n].clone()
        } else {
            instant_comittee_user(n, stkstate)
        }
    }).collect::<Vec<_>>()
}
pub fn comittee_n(n: usize, comittee: &Vec<Vec<usize>>, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>) -> Vec<usize> {
    if n < comittee.len() {
        comittee[n].clone()
    } else {
        instant_comittee(n, stkstate)
    }
}
pub fn instant_comittee(n: usize, stkstate: &VecHashMap<CompressedRistretto,(u64,Option<SocketAddr>)>) -> Vec<usize> {
    let y = stkstate.vec.par_iter().map(|(_,y)| y.0 as u128).collect::<Vec<_>>();
    let tot_stk: u128 = y.par_iter().sum(); /* initial queue will be 0 for all non0 shards... */

    let mut s = AHasher::new_with_keys(n as u128, n as u128);
    s.write(&format!("shard_takeover-{}",n).as_bytes());
    s.write(&stkstate.vec.par_iter().map(|x| x.0.to_bytes()).flatten().collect::<Vec<_>>());
    (0..NUMBER_OF_VALIDATORS).map(|x| {
        let mut s = s.clone();
        s.write(&x.to_le_bytes()[..]);
        let c = s.finish() as u128;

        let mut staker = (c%tot_stk) as i128;
        let mut w = 0;
        for (i,&j) in y.iter().enumerate() {
            staker -= j as i128;
            if staker <= 0
                {w = i; break;}
        };
        w
    }).collect::<Vec<_>>()
}
pub fn comittee_n_user(n: usize, comittee: &Vec<Vec<usize>>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Vec<usize> {
    if n < comittee.len() {
        comittee[n].clone()
    } else {
        instant_comittee_user(n, stkstate)
    }
}
pub fn instant_comittee_user(n: usize, stkstate: &Vec<(CompressedRistretto,u64)>) -> Vec<usize> {
    let y = stkstate.par_iter().map(|(_,y)| *y as u128).collect::<Vec<_>>();
    let tot_stk: u128 = y.par_iter().sum(); /* initial queue will be 0 for all non0 shards... */

    let mut s = AHasher::new_with_keys(n as u128, n as u128);
    s.write(&format!("shard_takeover-{}",n).as_bytes());
    s.write(&stkstate.par_iter().map(|x| x.0.to_bytes()).flatten().collect::<Vec<_>>());
    (0..NUMBER_OF_VALIDATORS).map(|x| {
        let mut s = s.clone();
        s.write(&x.to_le_bytes()[..]);
        let c = s.finish() as u128;

        let mut staker = (c%tot_stk) as i128;
        let mut w = 0;
        for (i,&j) in y.iter().enumerate() {
            staker -= j as i128;
            if staker <= 0
                {w = i; break;}
        };
        w
    }).collect::<Vec<_>>()
}
pub fn refill_user(queue: &mut Vec<VecDeque<usize>>, exitqueue: &mut Vec<VecDeque<usize>>, comittee: &mut Vec<Vec<usize>>, stkstate: &Vec<(CompressedRistretto,u64)>) {

    let y = stkstate.par_iter().map(|(_,y)| *y as u128).collect::<Vec<_>>();
    let tot_stk: u128 = y.par_iter().sum(); /* initial queue will be 0 for all non0 shards... */

    queue.par_iter_mut().enumerate().for_each(|(shard,x)| {
        if x.len() < QUEUE_LENGTH {
            let mut s = AHasher::new_with_keys(shard as u128, shard as u128);
            s.write(b"refill_queue");
            s.write(&x.iter().map(|x| x.to_le_bytes()).flatten().collect::<Vec<_>>());
            if x.len() == 0 {
                *x = (0..QUEUE_LENGTH).map(|x| {
                    let mut s = s.clone();
                    s.write(&x.to_le_bytes()[..]);
                    let c = s.finish() as u128;

                    let mut staker = (c%tot_stk) as i128;
                    let mut w = 0;
                    for (i,&j) in y.iter().enumerate() {
                        staker -= j as i128;
                        if staker <= 0
                            {w = i; break;}
                    };
                    w
                }).collect::<VecDeque<_>>();
            } else {
                x.par_extend((0..QUEUE_LENGTH-x.len()).map(|y| {
                    let mut s = s.clone();
                    s.write(&y.to_le_bytes()[..]);
                    let c = s.finish() as usize;

                    x[c%x.len()]
                }).collect::<Vec<_>>());
            }
        }
    });
    exitqueue.par_iter_mut().enumerate().for_each(|(shard,x)| {
        if x.len() < QUEUE_LENGTH {
            let mut s = AHasher::new_with_keys(shard as u128, shard as u128);
            s.write(b"refill_exitqueue");
            s.write(&x.iter().map(|x| x.to_le_bytes()).flatten().collect::<Vec<_>>());
            if x.len() == 0 {
                *x = (0..QUEUE_LENGTH).map(|x| {
                    let mut s = s.clone();
                    s.write(&x.to_le_bytes()[..]);
                    let c = s.finish() as u128;

                    let mut staker = (c%tot_stk) as i128;
                    let mut w = 0;
                    for (i,&j) in y.iter().enumerate() {
                        staker -= j as i128;
                        if staker <= 0
                            {w = i; break;}
                    };
                    w
                }).collect::<VecDeque<_>>();
            } else {
                x.par_extend((0..QUEUE_LENGTH-x.len()).map(|y| {
                    let mut s = s.clone();
                    s.write(&y.to_le_bytes()[..]);
                    let c = s.finish() as usize;

                    x[c%x.len()]
                }).collect::<Vec<_>>());
            }
        }
    });
    comittee.par_iter_mut().enumerate().for_each(|(shard,x)| {
        if x.len() < NUMBER_OF_VALIDATORS {
            let mut s = AHasher::new_with_keys(shard as u128, shard as u128);
            s.write(b"refill_comittee");
            s.write(&x.iter().map(|x| x.to_le_bytes()).flatten().collect::<Vec<_>>());
            if x.len() == 0 {
                *x = (0..NUMBER_OF_VALIDATORS).map(|x| {
                    let mut s = s.clone();
                    s.write(&x.to_le_bytes()[..]);
                    let c = s.finish() as u128;

                    let mut staker = (c%tot_stk) as i128;
                    let mut w = 0;
                    for (i,&j) in y.iter().enumerate() {
                        staker -= j as i128;
                        if staker <= 0
                            {w = i; break;}
                    };
                    w
                }).collect::<Vec<_>>();
            } else {
                x.par_extend((0..NUMBER_OF_VALIDATORS-x.len()).map(|y| {
                    let mut s = s.clone();
                    s.write(&y.to_le_bytes()[..]);
                    let c = s.finish() as usize;

                    x[c%x.len()]
                }).collect::<Vec<_>>());
            }
        }
    });

}







pub struct History {}

static FILE_NAME: &str = "history";

/// this represents a file that saves the public keys and commitments of all the OTAccounts that have appeared on the block chain (it is used to verify transactions and generate rings)
impl History {
    /// Create the file
    pub fn initialize() {
        File::create(FILE_NAME).unwrap();
    }

    /// Get the information on the OTAccount at height location as compressed Ristrettos
    pub fn get(location: &u64) -> [CompressedRistretto;2] {
        let mut byte = [0u8;64];
        let mut r = BufReader::new(File::open(FILE_NAME).unwrap());
        r.seek(SeekFrom::Start(location*64)).expect("Seek failed");
        r.read(&mut byte).unwrap();
        [CompressedRistretto::from_slice(&byte[..32]),CompressedRistretto::from_slice(&byte[32..])] // OTAccount::summon_ota() from there
    }

    /// Get the information on the OTAccount at height location as raw bytes
    pub fn get_raw(location: &u64) -> [u8; 64] {
        let mut bytes = [0u8;64];
        let mut r = BufReader::new(File::open(FILE_NAME).unwrap());
        r.seek(SeekFrom::Start(location*64)).expect("Seek failed");
        r.read(&mut bytes).unwrap();
        bytes
    }

    /// Generate a OTAccount from the raw bytes
    pub fn read_raw(bytes: &[u8; 64]) -> OTAccount { // assumes the bytes start at the beginning
        OTAccount::summon_ota(&[CompressedRistretto::from_slice(&bytes[..32]),CompressedRistretto::from_slice(&bytes[32..64])]) // OTAccount::summon_ota() from there
    }

    /// Appends new OTAccounts to the file
    pub fn append(accs: &Vec<OTAccount>) {
        let buf = accs.into_iter().map(|x| [x.pk.compress().as_bytes().to_owned(),x.com.com.compress().as_bytes().to_owned()].to_owned()).flatten().flatten().collect::<Vec<u8>>();
        let mut f = OpenOptions::new().append(true).open(FILE_NAME).unwrap();
        f.write_all(&buf.iter().map(|x|*x).collect::<Vec<u8>>()).unwrap();

    }
}







#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crate::{constants::PEDERSEN_H, vechashmap::VecHashMap};



    #[test]
    fn message_signing_test() {
        use curve25519_dalek::scalar::Scalar;
        use crate::validation::Signature;

        let message = "hi!!!".as_bytes().to_vec();
        let sk = Scalar::from(rand::random::<u64>());
        let pk = (sk*PEDERSEN_H()).compress();
        let mut m = Signature::sign_message2(&sk,&message);
        assert!(Signature::recieve_signed_message2(&mut m).unwrap() == pk);
        assert!(m == message);
    }

    #[test]
    fn message_signing_nonced_test() {
        use curve25519_dalek::scalar::Scalar;
        use crate::validation::Signature;

        let message = "hi!!!".as_bytes().to_vec();
        let sk = Scalar::from(rand::random::<u64>());
        let pk = (sk*PEDERSEN_H()).compress();
        let mut stkstate = VecHashMap::new();

        stkstate.insert(pk,(9012309183u64,None));
        let mut m = Signature::sign_message_nonced(&sk,&message,&0usize,&80u64);
        assert!(0 == Signature::recieve_signed_message_nonced(&mut m,&stkstate,&80u64).unwrap());
        assert!(m == message);
    }

    #[test]
    fn many_block_time_calculations() {
        
        let seconds_per_year = 31_536_000f64;

        let runtime = Instant::now();
        let mut t0 = 0f64;
        for _ in 0..1_000_000 {
            let t = 1f64/(2f64*t0+2f64).ln();
            t0 += t/seconds_per_year;
        }
        println!("runtime: {}ms",runtime.elapsed().as_millis());
    }
}