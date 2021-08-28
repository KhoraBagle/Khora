use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::ristretto::{RistrettoPoint, CompressedRistretto};
use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use rayon::iter::{IntoParallelIterator, ParallelIterator, IntoParallelRefIterator};
use std::hash::{Hash, Hasher};
use sha3::{Digest, Sha3_512};
use rand::{thread_rng, Rng};
use serde::{Serialize, Deserialize};

use crate::commitment::{Commitment};
use crate::lpke::Ciphertext;
use crate::constants::PEDERSEN_H;


#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum AccountError{
    NotOurAccount,
    NotPrivateAccount
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Account{
    sk: Scalar,
    pub pk: RistrettoPoint,
    ask: Scalar,
    pub apk: RistrettoPoint,
    vsk: Scalar,
    pub vpk: RistrettoPoint,
}


pub type Tag = CompressedRistretto;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OTAccount{ //i made all but first 2 public
    pub pk: RistrettoPoint,
    pub com: Commitment,
    pub account: Option<Account>,
    pub eek: Option<Ciphertext>,
    pub eck: Option<Ciphertext>,
    pub ek: Option<Vec<u8>>,
    pub s: Option<Scalar>,
    pub sk: Option<Scalar>, //made this public (so I could sign lightning)
    pub tag: Option<Tag>, //made this public
}

impl Eq for OTAccount {}
impl PartialEq for OTAccount{
    fn eq(&self, other: &Self) -> bool {
        self.pk == other.pk && self.com.com == other.com.com
    }
}

impl Default for OTAccount{
    fn default() -> Self {
        let mut csrng = thread_rng();

        OTAccount {
            pk: RistrettoPoint::random(&mut csrng),
            com: Commitment::default(),
            account: None,
            eek: None,
            eck: None,
            ek: None,
            s: None,
            sk: None,
            tag: None,
        }
    }
}

pub fn stakereader_acc() -> Account { // use this to check the to staker verifies
    Account{
        sk: Scalar::one(),
        pk: Account::tag_k_gen(Scalar::one()),
        ask: Scalar::one(),
        apk: RISTRETTO_BASEPOINT_POINT,
        vsk: Scalar::one(),
        vpk: RISTRETTO_BASEPOINT_POINT,
    }
}

pub fn fee_ota(amount: &Scalar) -> OTAccount { //makes the fee output account
    let com = Commitment::commit(amount, &Scalar::from(0u8));

    OTAccount{
        pk: RISTRETTO_BASEPOINT_POINT,
        com,
        ..Default::default()
    }
}

impl Account {

    pub fn tag_k_gen(x: Scalar) -> RistrettoPoint {
        x*PEDERSEN_H()
    }

    fn tag_eval(x: Scalar) -> RistrettoPoint {
        x.invert() * RISTRETTO_BASEPOINT_POINT
    }

    pub fn new(x: &String) -> Account { //makes accounts from a string
        let mut hasher = Sha3_512::new();
        hasher.update(&x.as_bytes());
        let sk = Scalar::from_hash(hasher.clone());
        hasher.update(&x.as_bytes());
        let ask = Scalar::from_hash(hasher.clone());
        hasher.update(&x.as_bytes());
        let vsk = Scalar::from_hash(hasher);
        Account{
            sk,
            pk: Account::tag_k_gen(sk),
            ask,
            apk: ask*RISTRETTO_BASEPOINT_POINT,
            vsk, // appendix B has info on ask (tsk in the paper) and vsk
            vpk: vsk*RISTRETTO_BASEPOINT_POINT,
        }
    }

    pub fn name(&self) -> String {
        std::str::from_utf8(&[self.pk,self.apk,self.vpk].into_par_iter().map(|key| {
            let key = key.compress();
            key.as_bytes().par_iter().map(|x| (x%16) + 97)
            .chain(key.as_bytes().par_iter().map(|x| (x/16) + 97).collect::<Vec<u8>>()).collect::<Vec<u8>>()
        }).flatten().collect::<Vec<u8>>()).unwrap().to_string()
    }

    pub fn from_pks(pk: &CompressedRistretto,apk: &CompressedRistretto,vpk: &CompressedRistretto) -> Self {
        Account{
            sk: Scalar::zero(),
            pk: pk.decompress().unwrap(),
            ask: Scalar::zero(),
            apk: apk.decompress().unwrap(),
            vsk: Scalar::zero(),
            vpk: vpk.decompress().unwrap(),
        }        
    }

    pub fn derive_ot(&self, amount: &Scalar) -> OTAccount{
        let mut csprng = thread_rng();
        let randomness = Scalar::random(&mut csprng);
        let com = Commitment::commit(amount, &randomness);
        let contains = ( *amount, randomness);
        let serialized = bincode::serialize(&contains).unwrap();
        let ek = thread_rng().gen::<[u8; 32]>();
        // println!("{:?}",ek);
        let mut hasher = Sha3_512::new();
        hasher.update(&self.pk.compress().as_bytes());
        hasher.update(&ek);
        let s = Scalar::from_hash(hasher);
        let pk = self.pk + Account::tag_k_gen(s);

        let mut label = pk.compress().as_bytes().to_vec();
        label.extend( com.com.compress().as_bytes().to_vec());
        let eek = Ciphertext::encrypt(&self.apk, &label, &ek.to_vec());
        let eck = Ciphertext::encrypt(&self.vpk, &label, &serialized);

        OTAccount{
            pk,
            com,
            account: Some(*self),
            ek: Some(ek.to_vec()),
            eek: Some(eek),
            eck: Some(eck),
            ..Default::default()
        }
    }
    /* i could probably delete all of eek if i wanted to */

    pub fn receive_ot(&self, acc: &OTAccount) -> Result<OTAccount, AccountError> {
        let mut label = acc.pk.compress().as_bytes().to_vec();
        label.extend( acc.com.com.compress().as_bytes().to_vec());
        let ek = match acc.eek.as_ref().unwrap().decrypt(&self.ask, &label) {
            Ok(ek) => ek, // tracking secret key (can scan for tx)
            Err(_) => return Err(AccountError::NotOurAccount)
        };
        let ck = match acc.eck.as_ref().unwrap().decrypt(&self.vsk, &label) {
            Ok(ck) => ck,  // viewing secret key (can also view amount)
            Err(_) => return Err(AccountError::NotOurAccount)
        };
        let (amount, randomness): (Scalar, Scalar) = bincode::deserialize(&ck).unwrap();

        let mut hasher = Sha3_512::new();
        hasher.update(&self.pk.compress().as_bytes());
        hasher.update(&ek);
        let s = Scalar::from_hash(hasher);
        let sk = self.sk + s;

        if Account::tag_k_gen(sk) != acc.pk {
            return Err(AccountError::NotOurAccount)
        }
        let trcom = Commitment::commit(&amount, &randomness);
        if trcom != acc.com {
            return Err(AccountError::NotOurAccount)
        }
        
        Ok(OTAccount{
            pk: acc.pk,
            com: trcom,
            account: Some(*self),
            ek: Some(ek),
            eek: acc.eek.clone(),
            eck: acc.eck.clone(),
            s: Some(s),
            sk: Some(sk),
            tag: Some(Account::tag_eval(sk).compress()),
        })
    }

    pub fn read_ot(&self, acc: &OTAccount) -> Result<OTAccount, AccountError> {
        let mut label = acc.pk.compress().as_bytes().to_vec();
        label.extend( acc.com.com.compress().as_bytes().to_vec());
        let ck = match acc.eck.as_ref().unwrap().decrypt(&self.vsk, &label) {
            Ok(ck) => ck,  // viewing secret key (can also view amount)
            Err(_) => return Err(AccountError::NotOurAccount)
        };
        
        let (amount, randomness): (Scalar, Scalar) = bincode::deserialize(&ck).unwrap();
        let trcom = Commitment::commit(&amount, &randomness);

        Ok(OTAccount{
            pk: acc.pk,
            com: trcom,
            account: acc.account.to_owned(),
            ek: acc.ek.to_owned(),
            eek: acc.eek.to_owned(),
            eck: acc.eck.to_owned(),
            s: acc.s.to_owned(),
            sk: acc.sk.to_owned(),
            tag: acc.tag.to_owned(),
        })
    }

    pub fn derive_stk_ot(&self, amount: &Scalar) -> OTAccount{
        let randomness = Scalar::from(0u8);
        let com = Commitment::commit(amount, &randomness);
        let contains = ( *amount, randomness);
        let serialized = bincode::serialize(&contains).unwrap();
        let ek = [0u8; 32]; // <----- if you want OT stk account, set this to something else (thugh they can't actually associate it with you)
        // anyone with a the viewing key can read this so it being zeros doesnt make it less secure
        
        let mut hasher = Sha3_512::new();
        hasher.update(&self.pk.compress().as_bytes());
        hasher.update(&ek);
        let s = Scalar::from_hash(hasher);
        let pk = self.pk + Account::tag_k_gen(s);

        let mut label = pk.compress().as_bytes().to_vec();
        label.extend( com.com.compress().as_bytes().to_vec());
        let eek = Ciphertext::encrypt(&self.apk, &label, &ek.to_vec());
        let eck = Ciphertext::encrypt(&self.vpk, &label, &serialized);

        OTAccount{
            pk,
            com,
            account: Some(*self),
            ek: Some(ek.to_vec()),
            eek: Some(eek),
            eck: Some(eck),
            ..Default::default()
        }
    }

    pub fn stake_acc(&self) -> Account {
        let sk = self.sk;
        Account{
            sk,
            pk: Account::tag_k_gen(sk),
            ask: Scalar::one(),
            apk: RISTRETTO_BASEPOINT_POINT,
            vsk: Scalar::one(), // appendix B has info on ask (tsk in the paper) and vsk
            vpk: RISTRETTO_BASEPOINT_POINT,
        }
    }


}


impl OTAccount {

    pub fn get_s(&self) -> Result<Scalar, AccountError> {
        match &self.ek {
            Some(ek) => {
                let mut hasher = Sha3_512::new();
                hasher.update(&self.account.as_ref().unwrap().pk.compress().as_bytes());
                hasher.update(ek);
                Ok(Scalar::from_hash(hasher))
            }
            None => Err(AccountError::NotPrivateAccount)
        }

    }

    pub fn summon_ota(x: &[CompressedRistretto;2]) -> OTAccount {
        OTAccount{
            pk: x[0].decompress().unwrap(),
            com: Commitment{com: x[1].decompress().unwrap(),..Default::default()},
            eek: Some(Ciphertext::default()),
            eck: Some(Ciphertext::default()),
            ..Default::default()
        }
    }

    pub fn track_ot(&self, ask: &Scalar) -> bool {
        let mut label = self.pk.compress().as_bytes().to_vec();
        label.extend( self.com.com.compress().as_bytes().to_vec());
        match self.eek.as_ref().unwrap().decrypt(ask, &label) {
            Ok(_) => return true, // tracking secret key (can scan for tx)
            Err(_) => return false
        };
    }

    // pub fn add_money(&mut self, acc: &Account, add_amount: &Scalar) -> Result<(), AccountError> {
    //     let mut label = self.pk.compress().as_bytes().to_vec();
    //     self.com.com = self.com.com + add_amount*RISTRETTO_BASEPOINT_POINT;
    //     self.com.amount = Some(self.com.amount.unwrap() + add_amount);
    //     label.extend( (self.com.com-add_amount*RISTRETTO_BASEPOINT_POINT).compress().as_bytes().to_vec());
    //     // let ek = match self.eek.as_ref().unwrap().decrypt(&acc.ask, &label) {
    //     //     Ok(ek) => ek, // tracking secret key (can scan for tx)
    //     //     Err(_) => return Err(AccountError::NotOurAccount)
    //     // };
    //     let ek = [0u8; 32].to_vec();


    //     let mut label = acc.pk.compress().as_bytes().to_vec();
    //     label.extend( acc.com.com.compress().as_bytes().to_vec());
    //     let ck = match acc.eck.as_ref().unwrap().decrypt(&self.vsk, &label) {
    //         Ok(ck) => ck,  // viewing secret key (can also view amount)
    //         Err(_) => return Err(AccountError::NotOurAccount)
    //     };
    //     println!("hi");
    //     let ck = match self.eck.as_ref().unwrap().decrypt(&acc.vsk, &label) {
    //         Ok(ck) => ck,  // viewing secret key (can also view amount)
    //         Err(_) => return Err(AccountError::NotOurAccount)
    //     };
    //     let (amount, randomness): (Scalar, Scalar) = serde_cbor::from_slice(&ck).unwrap();
    //     // println!("hi");

    //     let mut hasher = Sha3_512::new();
    //     hasher.update(&acc.pk.compress().as_bytes());
    //     hasher.update(&ek);
    //     let s = Scalar::from_hash(hasher);
    //     let sk = acc.sk + s; // if the s's match, so does the rest

    //     if Account::tag_k_gen(sk) != self.pk {
    //         return Err(AccountError::NotOurAccount)
    //     }
    //     let trcom = Commitment::commit(&amount, &randomness);
    //     if trcom != self.com {
    //         return Err(AccountError::NotOurAccount)
    //     }
        
    //     *self = OTAccount{
    //             pk: self.pk,
    //             com: trcom,
    //             account: Some(*acc),
    //             ek: Some(ek),
    //             eek: self.eek.clone(),
    //             eck: self.eck.clone(),
    //             s: Some(s),
    //             sk: Some(sk),
    //             tag: Some(Account::tag_eval(sk).compress()),
    //         };
    //     Ok(())
    // }


    pub fn get_sk(&self) -> Result<Scalar, AccountError> {
        match self.sk {
            Some(sk) => Ok(sk),
            None => match self.s {
                Some(s) => Ok(self.account.as_ref().unwrap().sk + s),
                None => match self.get_s() {
                    Ok(s) => Ok(self.account.as_ref().unwrap().sk + s),
                    Err(e) => Err(e)
                }
            }
        }
    }

    pub fn get_tag(&self) -> Result<Tag, AccountError> {
        match self.tag {
            Some(tag) => Ok(tag),
            None => match self.sk {
                Some(sk) => Ok(Account::tag_eval(sk).compress()),
                None => match self.get_sk() {
                    Ok(sk) => Ok(Account::tag_eval(sk).compress()),
                    Err(e) => Err(e)
                }
            }
        }
    }
    pub fn get_pk(&self) -> RistrettoPoint {
        self.pk
    }

    pub fn publish_offer(&self) -> OTAccount {
        OTAccount{
            pk: self.pk,
            com: self.com.publish(),
            eek: self.eek.clone(),
            eck: self.eck.clone(),
            ..Default::default()
        }
    }
}

impl Hash for OTAccount {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pk.compress().hash(state);
    }
}













// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn create_account() {
//         let acct = Account::new();
//         let typ = RistrettoPoint::default();
//         let ota = acct.derive_ot(&Scalar::from(6u64));

//         let rcv = acct.receive_ot(&ota);

//         assert_eq!(rcv.unwrap().com.amount.unwrap(),Scalar::from(6u64));

//     }
// }