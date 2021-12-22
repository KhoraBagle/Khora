use std::collections::HashMap;
use std::hash::Hash;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator, IntoParallelIterator};
use serde::{Serialize, Serializer, Deserialize, ser::SerializeSeq};
use std::cmp::Eq;









#[derive(Default, Clone, Debug)]
/// a datastructure i may start using
pub struct VecHashMap<T> {
    pub vec: Vec<T>,
    pub hashmap: HashMap<T,usize>,
}

impl<T> VecHashMap<T> {
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
}

impl<T: Eq + Clone + Hash> VecHashMap<T> {
    /// makes a vechashmap pair from a vec
    pub fn from(vec: Vec<T>) -> Self {
        Self {
            vec: vec.clone(),
            hashmap: vec.into_iter().enumerate().map(|(v,k)| (k,v)).collect(),
        }
        
    }
    /// attempt to change an element
    /// returns if it was able to find that element
    pub fn change(&mut self, element: &T, newelement: T) -> bool {
        if let Some(x) = self.hashmap.remove(element) {
            self.hashmap.insert(newelement.clone(),x);
            self.vec[x] = newelement;
            true
        } else {
            false
        }
    }
    /// returns if the vec contains that element
    pub fn contains(&self, element: &T) -> Option<usize> {
        self.hashmap.get(element).copied()
    }
    /// returns if the element at the vec contains that element's index
    pub fn get_by_index(&self, index: usize) -> &T {
        &self.vec[index]
    }
    /// removes an element from the vec and hashmap
    /// make sure gone is sorted for this to be used
    pub fn remove_all(&mut self, gone: &Vec<usize>) {
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
    /// inserts an element in the vec and hashmap
    pub fn insert_all(&mut self, enter: &[T]) {
        for e in enter {
            self.insert(e.to_owned());
        }
    }
}
/// takes as input an index returns where it goes/ if it's deleted
/// this function goes with the VecHashMap
/// make sure gone is sorted for this to be used
pub fn follow(me: usize, gone: &Vec<usize>, height: usize) -> Option<usize> {
    let mut out = Some(me);
    if gone.contains(&me) {
        out = None;
    }
    let glen = gone.len();
    if let Some(j) = &mut out {
        let mut pos = height-1-*j;
        while pos < gone.len() {
            pos = height-1-gone[glen-1-pos];
        }
        *j = height-1-pos;
    }
    out
}

















fn is_aesending<T>(data: &[T]) -> bool
where
    T: Ord,
{
    data.windows(2).all(|w| w[0] < w[1])
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
    x.remove_all(&remove);
    println!("remove time: {}ms for {} removals",runtime.elapsed().as_millis(),remove.len());

    assert!(is_aesending(&remove));

    let runtime = std::time::Instant::now();
    (0..xlen).for_each(|i| {
        let me = follow(i,&remove, xlen);
        // println!("{:?},{:?}",x.hashmap.get(&y.vec[i]).copied(), me);
        assert!(x.hashmap.get(&y.vec[i]).copied() == me)
    });
    println!("follow time: {}ms for {} removals",runtime.elapsed().as_millis()/xlen as u128,remove.len());



    let changes: u64 = 1000;
    let runtime = std::time::Instant::now();
    (0..changes).for_each(|y| {
        let i = y as usize;
        assert!(x.vec[i] != y);
        x.change(&x.vec[i].clone(),y);
        assert!(x.vec[i] == y);
    });
    println!("change element time: {}ms for {} changes",runtime.elapsed().as_millis()/xlen as u128,changes);




}