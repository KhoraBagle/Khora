use std::{fs::File, io::Read, time::Instant, collections::VecDeque};
use rand::{Rng};
use std::fmt::Binary;


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
    let t = Instant::now();
    let mut rng = rand::thread_rng();
    let mut me = rng.gen_range(0,1000usize);
    let mut x = (0..1000usize).collect::<VecDeque<_>>();
    println!("{},{}",me,x[me]);
    shuffle(&mut x, &mut rng,&mut me);
    println!("{},{}",me,x[me]);
    println!("time = {}ms",t.elapsed().as_millis());

    println!("{:#064b}",me);
    if let Some(children) = get_children(me) {
        println!("{:#064b}",children[0]);
        println!("{:#064b}",children[1]);
    }
    println!("time = {}ms",t.elapsed().as_millis());
}