use std::{fs::File, io::Read, time::Instant};

use curve25519_dalek::scalar::Scalar;
use khora::{account::Account, validation::NextBlock};
use sha3::{Digest, Sha3_512};






/// this is different from the one in the gui (b then a then c)
fn get_pswrd(a: &String, b: &String, c: &String) -> Vec<u8> {
    println!("{}",b);
    println!("{}",a);
    println!("{}",c);
    let mut hasher = Sha3_512::new();
    hasher.update(&b.as_bytes());
    hasher.update(&a.as_bytes());
    hasher.update(&c.as_bytes());
    Scalar::from_hash(hasher).as_bytes().to_vec()
}

/// we'll hard code this into initial history so they users cant see the passwords
/// this is also the place to test code
fn main() {

    // let a = [1,2,3];
    // println!("{:?}",a[3..].to_vec());
    let person0 = get_pswrd(&"1234".to_string(),&"1234567".to_string(),&"12345".to_string());
    let leader = Account::new(&person0).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
    println!("{:?}",leader);


    let person1 = get_pswrd(&"4321".to_string(),&"7654321".to_string(),&"54321".to_string());
    let leader = Account::new(&person1).name();
    println!("{:?}",leader);

    let leader = Account::new(&person1).nonanony_acc().name();
    println!("nonanony: {:?}",leader);


    let person2 = get_pswrd(&"4321".to_string(),&"1234567".to_string(),&"12345".to_string());
    let leader = Account::new(&person2).name();
    println!("{:?}",leader);

    let leader = Account::new(&person2).nonanony_acc().name();
    println!("nonanony: {:?}",leader);





    // let person0 = Account::new(&person0);
    // let person1 = Account::new(&person1);

    // let x = (0..1000u64).map(|x| person0.derive_ot(&Scalar::from(x))).collect::<Vec<_>>();


    // let now = Instant::now();

    // let z = x.iter().filter(|z| {
    //     person1.read_ot(z).is_ok()
    // }).collect::<Vec<_>>();
    

    // println!("{}",now.elapsed().as_millis());
    // println!("{}",z.len());

}