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
        return Some([you1,you0])
    }
    None
}
fn get_real_children(me: usize, nodes: usize) -> [Option<usize>;2] {
    if let Some(mut c) = get_children(me) {
        while c[1] >= nodes {
            if let Some(p) = get_children(c[1]).map(|x|x[0]) {
                c[1] = p;
            } else {
                return [Some(c[0]),None]
            }
        }
        return [Some(c[0]),Some(c[1])]
    } else {
        return [None,None]
    }
}
fn main() {
    let mut rng = rand::thread_rng();
    let mut tx = [0u8;1024];
    rng.fill(&mut tx);

    let nodes = 768usize;
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

    println!("{:#064b} = {}",me,me);
    let children = get_real_children(me,nodes);
    if let Some(c) = children[0] {println!("{:#064b} = {}",c,c)};
    if let Some(c) = children[1] {println!("{:#064b} = {}",c,c)};
    println!("time = {}ms",t.elapsed().as_millis());
}