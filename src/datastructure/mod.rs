use std::collections::HashMap;
use std::hash::Hash;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator, IntoParallelIterator};
use serde::{Serialize, Deserialize};







fn is_aesending<T>(data: &[T]) -> bool
where
    T: Ord,
{
    data.windows(2).all(|w| w[0] < w[1])
}



#[derive(Default, Clone, Debug)]
/// a datastructure i may start using
pub struct VecHashMap<T> {
    pub vec: Vec<T>,
    pub hashmap: HashMap<T,usize>,
}

impl<T: std::cmp::Eq + Clone + Hash> VecHashMap<T> {
    /// returns an empty VecHashMap
    pub fn new() -> Self {
        VecHashMap {
            vec: Vec::<T>::new(),
            hashmap: HashMap::<T,usize>::new(),
        }
    }
    /// returns the number of elements held
    pub fn len(&self) -> usize {
        self.vec.len()
    }
    /// removes an element from the vec and hashmap
    /// make sure gone is sorted for this to be used
    pub fn remove(&mut self, gone: &Vec<usize>) {

        // assert!(is_aesending(gone));

        gone.iter().for_each(|&x| {
            self.hashmap.remove(&self.vec[x]);
        });

        let vlen = self.vec.len();
        let dlen = gone.len();
        for (i,&e) in gone.into_iter().rev().enumerate() {
            if let Some(i) = self.hashmap.get_mut(&self.vec[vlen-i-1]) {
                *i = e;
            };
            self.vec.swap(e,vlen-i-1)
        }
        self.vec.truncate(vlen - dlen);
        
    }
    /// inserts an element in the vec and hashmap
    pub fn insert(&mut self, enter: T) {

        self.hashmap.insert(enter.clone(),self.vec.len());
        self.vec.push(enter);
        
    }
}
/// takes as input an index and moves/deletes it
/// this function goes with the vechashmap
/// make sure gone is sorted for this to be used
pub fn follow(me: &mut Option<usize>, gone: &Vec<usize>, height: usize) {
    if let Some(x) = &me {
        if gone.contains(x) {
            *me = None;
        }
    }
    let glen = gone.len();
    if let Some(j) = me {
        let mut pos = height-1-*j;
        while pos < gone.len() {
            pos = height-1-gone[glen-1-pos];
        }
        *j = height-1-pos;
    }
}

















#[test]
fn check_and_time() {
    

    


    let mut x = VecHashMap::<u64>::new();



    let mut rng = rand::thread_rng();
    (0..100000).for_each(|_| {
        x.insert(rand::Rng::gen(&mut rng));
    });
    // println!("{:#?}",x);
    let y = x.clone();
    let xlen = x.len();

    let mut remove = Vec::<usize>::new();
    (0..10000).for_each(|_| {
        let r = rand::Rng::gen::<usize>(&mut rng);
        remove.push(r%x.vec.len());
    });


    let runtime = std::time::Instant::now();
    remove.sort();
    remove.dedup();
    x.remove(&remove);
    println!("remove time: {}ms for {} removals",runtime.elapsed().as_millis(),remove.len());

    assert!(is_aesending(&remove));

    let runtime = std::time::Instant::now();
    (0..xlen).for_each(|i| {
        let mut me = Some(i);
        follow(&mut me,&remove, xlen);
        // println!("{:?},{:?}",x.hashmap.get(&y.vec[i]).copied(), me);
        assert!(x.hashmap.get(&y.vec[i]).copied() == me)
    });
    println!("follow time: {}ms for {} removals",runtime.elapsed().as_millis()/xlen as u128,remove.len());




}