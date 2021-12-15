use curve25519_dalek::ristretto::{CompressedRistretto};
use curve25519_dalek::scalar::Scalar;
use crate::account::*;
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
use std::io::{Seek, SeekFrom, BufReader};//, BufWriter};


pub static VERSION: &str = "v0.94";


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
pub const TOTAL_KHORA: f64 = 1.0e16;
/// bloom file for stakers
pub const STAKER_BLOOM_NAME: &'static str = "all_tags";
/// bloom file for stakers size
pub const STAKER_BLOOM_SIZE: usize =  1_000_000_000*4;
/// bloom file for stakers hashes
pub const STAKER_BLOOM_HASHES: u8 = 13;
/// if you have to many tx, you should combine them
pub const ACCOUNT_COMBINE: usize = 10;



/// calculates the reward for the current block
pub fn reward(cumtime: f64, blocktime: f64) -> f64 {
    (1.0/(1.653439E-6*cumtime + 1.0) - 1.0/(1.653439E-6*(cumtime + blocktime) + 1.0))*TOTAL_KHORA
}
/// calculates the amount of time the current block takes to be created
pub fn blocktime(cumtime: f64) -> f64 {
    // 60f64/(6.337618E-8f64*cumtime+2f64).ln()
    10.0
}

#[derive(Default, Clone, Serialize, Deserialize, Eq, Hash, Debug)]
/// the information on the transactions made that is saved in lightning blocks
/// in a minimalist setting where validators dont ever need to sync anyone, they only need to save their history object and the stake state at the current time
pub struct Syncedtx{
    pub stkout: Vec<u64>,
    pub stkin: Vec<(CompressedRistretto,u64)>,
    pub txout: Vec<OTAccount>,
    pub tags: Vec<CompressedRistretto>,
    pub fees: u64,
}

impl PartialEq for Syncedtx {
    fn eq(&self, other: &Self) -> bool {
        self.stkout == other.stkout && self.stkin == other.stkin && self.txout == other.txout && self.tags == other.tags && self.fees == other.fees
    }
}

impl Syncedtx {
    /// distills the important information from a group of transactions
    pub fn from(txs: &Vec<PolynomialTransaction>)->Syncedtx {
        let stkout = txs.iter().filter_map(|x|
            if x.inputs.last() == Some(&1) {Some(x.inputs.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap())).collect::<Vec<_>>())} else {None}
        ).flatten().collect::<Vec<u64>>();
        let stkin = txs.iter().map(|x|
            x.outputs.iter().filter_map(|y| 
                if let Ok(z) = stakereader_acc().read_ot(y) {Some((z.pk.compress(),u64::from_le_bytes(z.com.amount.unwrap().as_bytes()[..8].try_into().unwrap())))} else {None}
            ).collect::<Vec<_>>()
        ).flatten().collect::<Vec<(CompressedRistretto,u64)>>();
        let txout = txs.into_iter().map(|x|
            x.outputs.to_owned().into_iter().filter(|x| stakereader_acc().read_ot(x).is_err()).collect::<Vec<_>>()
        ).flatten().collect::<Vec<OTAccount>>();
        let tags = txs.iter().filter_map(|x|
            if x.inputs.last() != Some(&1) {Some(x.tags.clone())} else {None}
        ).flatten().collect::<Vec<CompressedRistretto>>();
        let fees = txs.iter().map(|x|x.fee).sum::<u64>();
        Syncedtx{stkout,stkin,txout,tags,fees}
    }

    /// the message block creaters sign so even lightning blocks can be verified
    pub fn to_sign(txs: &Vec<PolynomialTransaction>)->Vec<u8> {
        bincode::serialize(&Syncedtx::from(txs)).unwrap()
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
    pub fn to_signature(&self, validator_pool: &Vec<u64>) -> Signature {
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
    pub pk: u64,
}

impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.c == other.c && self.r == other.r && self.pk == other.pk
    }
}

impl Signature {
    /// replaces the public key location with the location in the comittee to save 7 bytes (not used because it's messier with little reward)
    /// this hasn't actually been implimented in the blocks
    pub fn to_validator_signature(&self, validator_pool: &Vec<u64>) -> ValidatorSignature {
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
    pub fn sign(key: &Scalar, message: &mut Sha3_512, location: &u64) -> Signature { // the inputs are the hashed messages you are checking for signatures on because it's faster for many messages.
        let mut csprng = thread_rng();
        let a = Scalar::random(&mut csprng);
        message.update((a*PEDERSEN_H()).compress().to_bytes());
        let c = Scalar::from_hash(message.to_owned());
        Signature{c, r: (a - c*key), pk: *location}
    }

    /// verifies a schoore signature on a message given as a Sha3_512 state
    pub fn verify(&self, message: &mut Sha3_512, stkstate: &Vec<(CompressedRistretto,u64)>) -> bool { // the inputs are the hashed messages you are checking for signatures on because it's faster for many messages.
        if self.pk as usize >= stkstate.len() {return false}
        message.update((self.r*PEDERSEN_H() + self.c*stkstate[self.pk as usize].0.decompress().unwrap()).compress().to_bytes());
        self.c == Scalar::from_hash(message.to_owned())
    }

    /// signs any message with your staker public key
    pub fn sign_message(key: &Scalar, message: &Vec<u8>, location: &u64) -> Vec<u8> {
        let mut s = Sha3_512::new();
        s.update(&message); // impliment non block check stuff for signatures
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

    /// recieves a signed message
    pub fn recieve_signed_message(signed_message: &mut Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Option<u64> {
        let sig = signed_message.par_drain(..72).collect::<Vec<_>>();
        let s = Signature{
            c: Scalar::from_bits(sig[..32].try_into().unwrap()),
            r: Scalar::from_bits(sig[32..64].try_into().unwrap()),
            pk: u64::from_le_bytes(sig[64..72].try_into().unwrap())
        };
        
        let mut h = Sha3_512::new();
        h.update(signed_message);
        if s.verify(&mut h, stkstate) {
            Some(s.pk)
        } else {
            None
        }
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
        if signed_message.len() < 96 {
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
    pub fn sign_message_nonced(key: &Scalar, message: &Vec<u8>, location: &u64, bnum: &u64) -> Vec<u8> {
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
    pub fn recieve_signed_message_nonced(signed_message: &mut Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>, bnum: &u64) -> Option<u64> {
        if signed_message.len() < 72 {return None}
        let sig = signed_message.par_drain(..72).collect::<Vec<_>>();
        let s = Signature{
            c: Scalar::from_bits(sig[..32].try_into().unwrap()),
            r: Scalar::from_bits(sig[32..64].try_into().unwrap()),
            pk: u64::from_le_bytes(sig[64..72].try_into().unwrap())
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
    pub shards: Vec<u16>,
    pub bnum: u64,
}
impl PartialEq for NextBlock {
    fn eq(&self, other: &Self) -> bool {
        bincode::serialize(self).unwrap() == bincode::serialize(other).unwrap()
    }
}
impl NextBlock {
    /// selects the transactions that are valid (as a member of the comittee in block generation)
    pub fn valicreate(key: &Scalar, location: &u64, leader: &CompressedRistretto, mut txs: Vec<PolynomialTransaction>, pool: &u16, bnum: &u64, last_name: &Vec<u8>, bloom: &BloomFile,/* _history: &Vec<OTAccount>,*/ stkstate: &Vec<(CompressedRistretto,u64)>) -> NextBlock {
        let stks = txs.par_iter().filter_map(|x| 
            if x.inputs.last() == Some(&1) {if x.verifystk(&stkstate).is_ok() {Some(x.to_owned())} else {None}} else {None}
        ).collect::<Vec<PolynomialTransaction>>(); /* i would use drain_filter but its unstable */
        let mut stks = stks.par_iter().enumerate().filter(|(i,x)| {
            x.inputs.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap())).collect::<Vec<_>>().par_iter().all(|&x|
                !stks[..*i].par_iter().flat_map(|x| x.inputs.par_chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap())).collect::<Vec<_>>()).collect::<Vec<u64>>()
                .contains(&x)
            )
        }).map(|x| x.1.to_owned()).collect::<Vec<_>>();
        txs.retain(|x| x.inputs.last() == Some(&0));
        
        
        
        let mut txs =
            txs.par_iter().enumerate().filter_map(|(i,x)| {
                if 
                x.tags.par_iter().all(|&x| !txs[..i].iter().flat_map(|x| x.tags.clone()).collect::<HashSet<CompressedRistretto>>().contains(&x))
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


        let m = vec![leader.to_bytes().to_vec(),bincode::serialize(&vec![pool]).unwrap(),Syncedtx::to_sign(&txs),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);
        NextBlock {
            validators: vec![],
            leader: Signature::sign(&key,&mut s,&location),
            txs: txs.to_owned(),
            last_name: last_name.to_owned(),
            shards: vec![*pool],
            bnum: *bnum,
        }
    }

    /// creates a full block from a collection of signatures in the comittee
    pub fn finish(key: &Scalar, location: &u64, sigs: &Vec<NextBlock>, validator_pool: &Vec<u64>, pool: &u16, bnum: &u64, last_name: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Result<NextBlock,&'static str> { // <----do i need to reference previous block explicitly?
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
                let m = vec![leader.clone(),bincode::serialize(&vec![pool]).unwrap(),Syncedtx::to_sign(&sigfinale[0].txs),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
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
                let m = vec![bincode::serialize(&vec![pool]).unwrap(),bnum.to_le_bytes().to_vec(),last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
                let mut s = Sha3_512::new();
                s.update(&m);
                let leader = Signature::sign(&key, &mut s,&location);
                if validator_pool.par_iter().filter(|x| !sigfinale.par_iter().all(|y| x.to_owned() != &y.leader.pk)).count() > SIGNING_CUTOFF {
                    return Ok(NextBlock{validators: sigs, leader, txs: sigfinale[0].txs.to_owned(), last_name: last_name.to_owned(), shards: vec![*pool], bnum: bnum.to_owned()})
                } else {
                    print!("not enough sigs... ");
                    break
                }
            }
        }
        println!("failed to make block :(");
        return Err("Not enoguh true sigs")
    }

    /// creates the final block from the collection of subblocks and signatures from the main shard
    /// WARNING:: MUST MAKE SURE blks[0] IS THE ONE YOU MADE YOURSELF
    pub fn valimerge(key: &Scalar, location: &u64, leader: &CompressedRistretto, blks: &Vec<NextBlock>, val_pools: &Vec<Vec<u64>>, bnum: &u64, last_name: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>, _mypoolnum: &u16) -> Signature {
        
        
        let mut blks: Vec<NextBlock> = blks.par_iter().zip(val_pools).filter_map(|(x,y)| if x.verify(&y,&stkstate).is_ok() {Some(x.to_owned())} else {None}).collect();
        let mut blk = blks.remove(0); // their own shard should be this one! (main shard should be contributing as a shard while waiting)
        let mut tags = blk.txs.par_iter().map(|x| x.tags.clone()).flatten().collect::<HashSet<Tag>>();
        for mut b in blks {
            if b.bnum == *bnum {
                b.txs = b.txs.into_par_iter().filter(|t| {
                    // tags.is_disjoint(&HashSet::from_par_iter(t.tags.par_iter().cloned()))
                    t.tags.par_iter().all(|x| !tags.contains(x))
                }).collect::<Vec<PolynomialTransaction>>();
                blk.txs.par_extend(b.txs.clone());
                let x = b.txs.len();
                tags = tags.union(&b.txs.into_par_iter().map(|x| x.tags).flatten().collect::<HashSet<Tag>>()).map(|&x| x).collect::<HashSet<CompressedRistretto>>();
                if x > 63 {
                    blk.shards.par_extend(b.shards);
                }
            }
        }


        let m = vec![leader.to_bytes().to_vec(),bincode::serialize(&blk.shards).unwrap(),Syncedtx::to_sign(&blk.txs),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);
        Signature::sign(&key,&mut s, &location)
    }

    /// verifies a full block (that the comittee acted as they should)
    pub fn finishmerge(key: &Scalar, location: &u64, sigs: &Vec<Signature>, blks: &Vec<NextBlock>, val_pools: &Vec<Vec<u64>>, headpool: &Vec<u64>, bnum: &u64, last_name: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>, _mypoolnum: &u16) -> NextBlock {
        let headpool = headpool.into_par_iter().map(|x|stkstate[*x as usize].0).collect::<Vec<CompressedRistretto>>();
        let mut blks: Vec<NextBlock> = blks.par_iter().zip(val_pools).filter_map(|(x,y)| if x.verify(&y, &stkstate).is_ok() {Some(x.to_owned())} else {None}).collect();
        let mut blk = blks.remove(0);
        
        let mut tags = blk.txs.par_iter().map(|x| x.tags.clone()).flatten().collect::<HashSet<Tag>>();
        for mut b in blks.into_iter() {
            b.txs = b.txs.into_par_iter().filter(|t| {
                t.tags.par_iter().all(|x| !tags.contains(x))
            }).collect::<Vec<PolynomialTransaction>>();
            blk.txs.par_extend(b.txs.clone());
            let x = b.txs.len();
            tags = tags.union(&b.txs.into_par_iter().map(|x| x.tags).flatten().collect::<HashSet<Tag>>()).map(|&x| x).collect::<HashSet<CompressedRistretto>>();
            // println!("tx: {}",x);
            if x > 63 {
                blk.shards.par_extend(b.shards);
            }
        }
        
        let leader = (key*PEDERSEN_H()).compress().as_bytes().to_vec();
        let m = vec![leader.clone(),bincode::serialize(&blk.shards).unwrap(),Syncedtx::to_sign(&blk.txs),bnum.to_le_bytes().to_vec(),last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);
        let sigs = sigs.into_par_iter().filter(|x|
            Signature::verify(x, &mut s.clone(), stkstate)
        ).collect::<Vec<&Signature>>();
        let sigs = sigs.into_par_iter().filter(|x| !headpool.clone().into_par_iter().all(|y| stkstate[x.pk as usize].0 != y)).collect::<Vec<&Signature>>();
        let sigcopy = sigs.clone();
        let sigs = sigs.into_par_iter().enumerate().filter_map(|(i,x)| if sigcopy[..i].par_iter().all(|y| x.pk != y.pk) {Some(x.to_owned())} else {None}).collect::<Vec<Signature>>();
        let mut s = Sha3_512::new();
        s.update(&bincode::serialize(&sigs).unwrap().to_vec());
        let c = s.finalize();

        let m = vec![bincode::serialize(&blk.shards).unwrap(),bnum.to_le_bytes().to_vec(), last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);
        let leader = Signature::sign(&key, &mut s, &location);
        NextBlock{validators: sigs, leader, txs: blk.txs, last_name: last_name.clone(), shards: blk.shards, bnum: bnum.to_owned()}
    }

    /// verifies a full block (that the comittee acted as they should)
    pub fn verify(&self, validator_pool: &Vec<u64>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Result<bool, &'static str> {
        let mut s = Sha3_512::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![bincode::serialize(&self.shards).unwrap(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);

        if !self.leader.verify(&mut s, &stkstate) {
            return Err("leader is fake")
        }
        let m = vec![stkstate[self.leader.pk as usize].0.as_bytes().to_vec().clone(),bincode::serialize(&self.shards).unwrap(),Syncedtx::to_sign(&self.txs),self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
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
        let x = self.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>();
        if !x.clone().par_iter().enumerate().all(|(i,y)| x[..i].par_iter().all(|z| y != z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }

    /// the function you use to pay the comittee if you are not saving the block
    pub fn pay_all_empty(shard: &usize, comittee: &Vec<Vec<usize>>, valinfo: &mut Vec<(CompressedRistretto,u64)>, reward: f64) {
        let winners = comittee[*shard].iter();
        let inflation = (reward/winners.len() as f64) as u64;
        for &i in winners {
            valinfo[i].1 += inflation;
        }
    }

    /// the function you use to pay yourself if you are not saving the block
    pub fn pay_self_empty(shard: &usize, comittee: &Vec<Vec<usize>>, mine: &mut Option<[u64;2]>, reward: f64) -> bool {

        let mut changed = false;
        if let Some(mine) = mine {
            let winners = comittee[*shard].iter();
            let inflation = (reward/winners.len() as f64) as u64;
            for &i in winners {
                if mine[0] == i as u64 {
                    changed = true;
                    mine[1] += inflation;
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
    pub fn tolightning(&self) -> LightningSyncBlock {
        LightningSyncBlock {
            validators: self.validators.to_owned(),
            leader: self.leader.to_owned(),
            info: Syncedtx::from(&self.txs),
            shards: self.shards.to_owned(),
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
    pub shards: Vec<u16>,
    pub bnum: u64,
    pub last_name: Vec<u8>,
}
impl LightningSyncBlock {
    /// verifies that the block is real and the 128 comittee members came to consensus. all computations are carried out in parallell
    pub fn verify_multithread(&self, validator_pool: &Vec<u64>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Result<bool, &'static str> {
        let mut s = Sha3_512::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![bincode::serialize(&self.shards).unwrap(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut h = Sha3_512::new();
        h.update(&m);
        if !self.leader.verify(&mut h, &stkstate) {
            return Err("leader is fake")
        }
        let m = vec![stkstate[self.leader.pk as usize].0.as_bytes().to_vec().clone(), bincode::serialize(&self.shards).unwrap(), bincode::serialize(&self.info).unwrap(), self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
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
        let x = self.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>();
        if x.par_iter().enumerate().any(|(i,y)| x.par_iter().take(i).any(|z| y == z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }

    /// verifies that the block is real and the 128 comittee members came to consensus
    pub fn verify(&self, validator_pool: &Vec<u64>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Result<bool, &'static str> {
        let mut s = Sha3_512::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![bincode::serialize(&self.shards).unwrap(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut h = Sha3_512::new();
        h.update(&m);
        if !self.leader.verify(&mut h, &stkstate) {
            return Err("leader is fake")
        }
        let m = vec![stkstate[self.leader.pk as usize].0.as_bytes().to_vec().clone(), bincode::serialize(&self.shards).unwrap(), bincode::serialize(&self.info).unwrap(), self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
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
        let x = self.validators.iter().map(|x| x.pk).collect::<Vec<u64>>();
        if x.iter().enumerate().any(|(i,y)| x.iter().take(i).any(|z| y == z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }

    /// updates the staker state by dulling out punishments and gifting rewards. also updates the queue, exitqueue, and comittee if stakers left
    pub fn scan_as_noone(&self, valinfo: &mut Vec<(CompressedRistretto,u64)>, netinfo: &mut HashMap<CompressedRistretto, (u64, Option<SocketAddr>)>, queue: &mut Vec<VecDeque<usize>>, exitqueue: &mut Vec<VecDeque<usize>>, comittee: &mut Vec<Vec<usize>>, reward: f64, save_history: bool) {
        if save_history {History::append(&self.info.txout)};

        let winners: Vec<usize>;
        let masochists: Vec<usize>;
        let lucky: Vec<usize>;
        let feelovers: Vec<usize>;
        let x = self.validators.iter().map(|x| x.pk as usize).collect::<HashSet<_>>();

        winners = comittee[self.shards[0] as usize].iter().filter(|&y| x.contains(y)).map(|x| *x).collect::<Vec<_>>();
        masochists = comittee[self.shards[0] as usize].iter().filter(|&y| !x.contains(y)).map(|x| *x).collect::<Vec<_>>();
        if self.shards.len() > 1 {
            feelovers = self.shards[1..].iter().map(|x| comittee[*x as usize].clone()).flatten().chain(winners.clone()).collect::<Vec<_>>();
        } else {
            feelovers = winners.clone();
        }
        lucky = comittee[*self.shards.iter().max().unwrap() as usize + 1].clone();
        let fees = self.info.fees/(feelovers.len() as u64);
        let inflation = (reward/winners.len() as f64) as u64;


        for i in winners {
            valinfo[i].1 += inflation;
        }
        for i in feelovers {
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




        let mut savelist = HashMap::new();
        for x in self.info.stkout.iter().rev() {
            savelist.insert(valinfo[*x as usize].0, netinfo.remove(&valinfo[*x as usize].0).expect("this IS in netinfo"));
            valinfo[*x as usize + 1..].iter().for_each(|(x,_)| {
                netinfo.get_mut(x).unwrap().0 -= 1;
            });
            valinfo.remove(*x as usize);
            *queue = queue.into_par_iter().map(|y| {
                let z = y.into_par_iter().filter_map(|z|
                    if *z > *x as usize {Some(*z - 1)}
                    else if *z == *x as usize {None}
                    else {Some(*z)}
                ).collect::<VecDeque<_>>();
                if z.len() == 0 {
                    VecDeque::from_iter([0usize])
                } else {
                    z
                }
            }).collect::<Vec<_>>();
            *exitqueue = exitqueue.into_par_iter().map(|y| {
                let z = y.into_par_iter().filter_map(|z|
                    if *z > *x as usize {Some(*z - 1)}
                    else if *z == *x as usize {None}
                    else {Some(*z)}
                ).collect::<VecDeque<_>>();
                if z.len() == 0 {
                    VecDeque::from_iter([0usize])
                } else {
                    z
                }
            }).collect::<Vec<_>>();
            *comittee = comittee.into_par_iter().map(|y| {
                let z = y.into_par_iter().filter_map(|z|
                    if *z > *x as usize {Some(*z - 1)}
                    else if *z == *x as usize {None}
                    else {Some(*z)}
                ).collect::<Vec<_>>();
                if z.len() == 0 {
                    vec![0usize]
                } else {
                    z
                }
            }).collect::<Vec<_>>();
        }

        queue.par_iter_mut().for_each(|x| {
            let mut s = Sha3_512::new();
            s.update(&bincode::serialize(&x).unwrap());
            let mut v = Scalar::from_hash(s.clone()).as_bytes().to_vec();
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            let mut y = (0..QUEUE_LENGTH-x.len()).map(|i| x[v[i] as usize%x.len()]).collect::<VecDeque<usize>>();
            x.append(&mut y);
        });
        exitqueue.par_iter_mut().for_each(|x| {
            let mut s = Sha3_512::new();
            s.update(&bincode::serialize(&x).unwrap());
            let mut v = Scalar::from_hash(s.clone()).as_bytes().to_vec();
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            let mut y = (0..QUEUE_LENGTH-x.len()).map(|i| x[v[i] as usize%x.len()]).collect::<VecDeque<usize>>();
            x.append(&mut y);
        });
        comittee.par_iter_mut().for_each(|x| {
            let mut s = Sha3_512::new();
            s.update(&bincode::serialize(&x).unwrap());
            let mut v = Scalar::from_hash(s.clone()).as_bytes().to_vec();
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            let mut y = (0..NUMBER_OF_VALIDATORS-x.len()).map(|i| x[v[i] as usize%x.len()]).collect::<Vec<usize>>();
            x.append(&mut y);
        });


        let realstkin = self.info.stkin.iter().filter(|x| {
            if let Some(&(i,_)) = netinfo.get(&x.0) {
                valinfo[i as usize].1 += x.1;
                return false
            }
            true
        }).copied().collect::<Vec<_>>();
        valinfo.extend(&realstkin);

        self.info.stkin.iter().enumerate().for_each(|(i,(x,_))| {
            if !netinfo.contains_key(x) {
                netinfo.insert(*x, ((i + valinfo.len()) as u64,None));
            }
        });

        for (x,v) in savelist {
            if let Some(val) = netinfo.get_mut(&x) {
                *val = v;
            }
        }

    }

    /// scans the block for money sent to you. additionally updates your understanding of the height. returns weather you recieved money
    pub fn scan(&self, me: &Account, mine: &mut HashMap<u64,OTAccount>, reversemine: &mut HashMap<CompressedRistretto,u64>, height: &mut u64, alltagsever: &mut HashSet<CompressedRistretto>) -> bool {
        let newmine = self.info.txout.par_iter().enumerate().filter_map(|(i,x)| if let Ok(y) = me.receive_ot(x) {Some((i as u64+*height,y))} else {None}).collect::<Vec<(u64,OTAccount)>>();
        let mut imtrue = !newmine.is_empty();
        for (n,m) in newmine {
            if alltagsever.contains(&m.tag.unwrap()) {
                println!("someone sent you money that you can't speand");
            } else {
                // let t = std::time::Instant::now();
                alltagsever.insert(m.tag.unwrap());
                // println!("insert tag: {}",t.elapsed().as_millis());
                // let t = std::time::Instant::now();
                reversemine.insert(m.tag.unwrap(),n);
                mine.insert(n,m);
                imtrue = false;
                // println!("recieve money: {}",t.elapsed().as_millis());
            }
        }
        for tag in self.info.tags.iter() {
            if let Some(loc) = reversemine.get(tag) {
                mine.remove(loc);
                reversemine.remove(tag);
                imtrue = false;
            }
        }

        *height += self.info.txout.len() as u64;

        imtrue
    }

    /// scans the block for any transactions sent to you or any rewards and punishments you recieved. it additionally updates the height of stakers. returns if your money changed then if your index changed
    pub fn scanstk(&self, me: &Account, mine: &mut Option<[u64;2]>, height: &mut u64, comittee: &Vec<Vec<usize>>, reward: f64, valinfo: &Vec<(CompressedRistretto,u64)>) -> (bool,bool) {

        // let t = std::time::Instant::now();


        let mut benone = false;
        let changed = mine.clone();
        let nonetosome = mine.is_none();
        if let Some(mine) = mine {
            let winners: Vec<usize>;
            let masochists: Vec<usize>;
            let lucky: Vec<usize>;
            let feelovers: Vec<usize>;
    
            let x = self.validators.iter().map(|x| x.pk as usize).collect::<HashSet<_>>();
    
            winners = comittee[self.shards[0] as usize].par_iter().filter(|&y| x.contains(y)).cloned().collect::<Vec<_>>();
            masochists = comittee[self.shards[0] as usize].par_iter().filter(|&y| !x.contains(y)).cloned().collect::<Vec<_>>();
            if self.shards.len() > 1 {
                feelovers = self.shards[1..].par_iter().map(|x| comittee[*x as usize].clone()).flatten().chain(winners.clone()).collect::<Vec<_>>();
            } else {
                feelovers = winners.clone();
            }
            lucky = comittee[*self.shards.iter().max().unwrap() as usize + 1].clone();
            
            let fees = self.info.fees/(feelovers.len() as u64);
            let inflation = (reward/winners.len() as f64) as u64;
    
            for i in winners {
                if mine[0] == i as u64 {
                    mine[1] += inflation;
                };
            }
            for i in feelovers {
                if mine[0] == i as u64 {
                    mine[1] += fees;
                };
            }
            let mut punishments = 0u64;
            for i in masochists {
                punishments += valinfo[i].1/PUNISHMENT_FRACTION;
                if mine[0] == i as u64 {
                    mine[1] -= valinfo[i].1/PUNISHMENT_FRACTION;
                }
            }
            punishments = punishments/lucky.len() as u64;
            for i in lucky {
                if mine[0] == i as u64 {
                    mine[1] += punishments;
                }
            }
    
    
        
    
    
    
            if self.info.stkout.contains(&mine[0]) {
                benone = true;
            }
            *height -= self.info.stkout.len() as u64;
            
            let stkcr = me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
            if benone {
                for (i,x) in self.info.stkin.iter().enumerate() {
                    if stkcr == x.0 {
                        *mine = [i as u64+*height,x.1];
                        benone = false;
                        break
                    }
                }
            } else {
                for x in self.info.stkin.iter() {
                    if stkcr == x.0 {
                        mine[1] += x.1;
                        break
                    }
                }
            }
            
            *height += self.info.stkin.len() as u64;

        } else {
            *height -= self.info.stkout.len() as u64;
            *height += self.info.stkin.len() as u64;
        }

        if benone {
            *mine = None;
        }
        // println!("{}",t.elapsed().as_millis());

        (changed != *mine, nonetosome && mine.is_some())


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
    pub fn update_bloom(&self,bloom:&BloomFile,parallel:&bool) {
        if *parallel {
            self.info.tags.par_iter().for_each(|x| bloom.insert(&x.as_bytes()));
        } else {
            self.info.tags.iter().for_each(|x| bloom.insert(&x.as_bytes()));
        }
    }




}


/// selects the stakers who get to validate the queue and exit_queue
pub fn select_stakers(block: &Vec<u8>, bnum: &u64, shard: &u128, queue: &mut VecDeque<usize>, exitqueue: &mut VecDeque<usize>, comittee: &mut Vec<usize>, stkstate: &Vec<(CompressedRistretto,u64)>) {
    let (_pool,y): (Vec<CompressedRistretto>,Vec<u128>) = stkstate.into_iter().map(|(x,y)| (x.to_owned(),*y as u128)).unzip();
    let tot_stk: u128 = y.iter().sum(); /* initial queue will be 0 for all non0 shards... */

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

    use crate::constants::PEDERSEN_H;



    #[test]
    fn message_signing_test() {
        use curve25519_dalek::scalar::Scalar;
        use crate::validation::Signature;

        let message = "hi!!!".as_bytes().to_vec();
        let sk = Scalar::from(rand::random::<u64>());
        let pk = (sk*PEDERSEN_H()).compress();
        let stkstate = vec![(pk,9012309183u64)];
        let mut m = Signature::sign_message(&sk,&message,&0u64);
        assert!(0 == Signature::recieve_signed_message(&mut m,&stkstate).unwrap());
        assert!(m == message);
    }

    #[test]
    fn message_signing_nonced_test() {
        use curve25519_dalek::scalar::Scalar;
        use crate::validation::Signature;

        let message = "hi!!!".as_bytes().to_vec();
        let sk = Scalar::from(rand::random::<u64>());
        let pk = (sk*PEDERSEN_H()).compress();
        let stkstate = vec![(pk,9012309183u64)];
        let mut m = Signature::sign_message_nonced(&sk,&message,&0u64,&80u64);
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