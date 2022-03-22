use std::{fs::File, io::Read, time::Instant, collections::VecDeque, convert::TryInto};
use rand::{Rng, prelude::StdRng, SeedableRng};
use std::fmt::Binary;
use std::num::Wrapping;


// An exact copy of rand::Rng::shuffle, with the signature modified to
// accept any type that implements LenAndSwap
fn shuffle<R, T>(values: &mut VecDeque<T>, mut rng: R, me: &mut usize)
    where R: rand::Rng {
    let mut i = values.len();
    let mut switch = 0;
    while i >= 2 {
        // invariant: elements with index >= i have been locked in place.
        i -= 1;
        switch = rng.gen_range(0, i + 1);
        // lock element i in place.
        if *me == switch {
            *me = i;
        } else if *me == i {
            *me = switch;
        }
        values.swap(i, switch);
    }
}

fn get_children(me: usize) -> Option<[usize;2]> {
    let ledz = me.trailing_zeros();
    if ledz != 0 {
        let indicator = 1 << ledz-1;
        let you0 = me | indicator;
        let you1 = you0 ^ (indicator << 1);
        return Some([you0,you1])
    }
    None
}
fn main() {
    let mut rng = rand::thread_rng();
    let mut tx = [0u8;1024];
    rng.fill(&mut tx);

    let nodes = 2u64.pow(10) as usize;
    println!("{} nodes",nodes);
    
    let mut me = rng.gen_range(0,nodes);
    let mut x = (0..nodes).collect::<VecDeque<_>>();
    println!("me = {}\tx[me] = {}",me,x[me]);

    let t = Instant::now();

    let csize = tx.len()/8;
    let numgen = tx.chunks_exact(csize).map(|c|c.into_iter().copied().reduce(|x,y| x ^ y).unwrap()).collect::<Vec<_>>();
    

    let mut rng = StdRng::seed_from_u64(u64::from_le_bytes(numgen.try_into().unwrap()));
    shuffle(&mut x, &mut rng,&mut me);
    println!("me = {}\tx[me] = {}",me,x[me]);
    println!("time = {}ms",t.elapsed().as_millis());

    println!("{:#064b}",me);
    if let Some(children) = get_children(me) {
        println!("{:#064b}",children[0]);
        println!("{:#064b}",children[1]);
    }
    println!("time = {}ms",t.elapsed().as_millis());
}