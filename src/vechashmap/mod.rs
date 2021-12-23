use std::collections::HashMap;
use std::hash::Hash;
use clap::Values;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator, IntoParallelIterator};
use serde::{Serialize, Serializer, Deserialize, ser::SerializeSeq};
use std::cmp::Eq;




pub trait RemoveAll {
    fn remove_all(&mut self, gone: &Vec<usize>);
}

impl<T>  RemoveAll for Vec<T> {

    /// removes elements from the vec in the O(1) way
    /// make sure gone is sorted as aesending for this to be used
    fn remove_all(&mut self, gone: &Vec<usize>) {
        // assert!(is_aesending(gone));

        let vlen = self.len();
        let dlen = gone.len();
        for (i,&e) in gone.into_iter().rev().enumerate() {
            self.swap(e,vlen-i-1)
        }
        self.truncate(vlen - dlen);
        
    }
}



#[derive(Default, Clone, Debug)]
/// vector that has O(1) lookup, modification, and deletion but does not preserve order...
/// elements can be accessed and identified by either their contents (group elements and extra info which is many bytes)
/// or their index (which can be specified by 8 bytes)
/// there's also a follow function to say where an index goes given what leaves and the height without knowing what's actually stored
/// (so users dont need to store the data structure)
pub struct VecHashMap<K,V> {
    /// the vector of the VecHashMap
    pub vec: Vec<(K,V)>,// maybe i should make this Vec<(K,V)> and HashMap<K,usize> so I can store extra info in K?
    /// the HashMap of the VecHashMap
    pub hashmap: HashMap<K,usize>,
}

impl<K,V> VecHashMap<K,V> {
    /// returns an empty VecHashMap
    pub fn new() -> Self {
        VecHashMap {
            vec: Vec::<(K,V)>::new(),
            hashmap: HashMap::<K,usize>::new(),
        }
    }
    /// returns the number of elements held
    pub fn len(&self) -> usize {
        self.vec.len()
    }
}

impl<K: Eq + Clone + Hash, V: Clone> VecHashMap<K,V> {
    /// makes a vechashmap pair from a vec
    pub fn from(vec: Vec<(K,V)>) -> Self {
        Self {
            vec: vec.clone(),
            hashmap: vec.into_iter().enumerate().map(|(v,k)| (k.0,v)).collect(),
        }
    }
    /// attempt to change a value given a key
    pub fn mut_value_from_key(&mut self, key: &K) -> &mut V {
        &mut self.vec[self.hashmap[key]].1
    }
    /// attempt to change a value given a key
    pub fn mut_value_from_index(&mut self, index: usize) -> &mut V {
        &mut self.vec[index].1
    }
    /// attempt to change a value given a key
    pub fn change_value_from_key(&mut self, key: &K, newvalue: V) {
        self.vec[self.hashmap[key]].1 = newvalue;
    }
    /// attempt to change a value given a key
    pub fn change_value_from_index(&mut self, index: usize, newvalue: V) {
        self.vec[index].1 = newvalue;
    }
    /// returns if the vec contains that element
    pub fn contains(&self, element: &K) -> Option<usize> {
        self.hashmap.get(element).copied()
    }
    /// returns the key value pair of the index's element
    pub fn get_by_index(&self, index: usize) -> &(K,V) {
        &self.vec[index]
    }
    /// returns the key value pair of the index's element
    pub fn get_index_by_key(&self, key: &K) -> Option<usize> {
        if let Some(&x)  =self.hashmap.get(key) {
            Some(x)
        } else {
            None
        }
    }
    /// removes an element from the vec and hashmap
    /// make sure gone is sorted as aesending for this to be used
    pub fn remove_all(&mut self, gone: &Vec<usize>) {
        // assert!(is_aesending(gone));

        gone.iter().for_each(|&x| {
            self.hashmap.remove(&self.vec[x].0);
        });

        let vlen = self.vec.len();
        let dlen = gone.len();
        for (i,&e) in gone.into_iter().rev().enumerate() {
            if let Some(i) = self.hashmap.get_mut(&self.vec[vlen-i-1].0) {
                *i = e;
            };
            self.vec.swap(e,vlen-i-1)
        }
        self.vec.truncate(vlen - dlen);
        
    }
    /// inserts an element in the vec and hashmap
    pub fn insert(&mut self, key: K, value: V) {

        self.hashmap.insert(key.clone(),self.vec.len());
        self.vec.push((key,value));
        
    }
    /// inserts an element in the vec and hashmap
    pub fn insert_all(&mut self, elements: &[(K,V)]) {
        for (k,v) in elements {
            self.insert(k.clone(),v.clone());
        }
    }
    /// modifies all values from keys by a function
    /// if a key is not valid, it is ignored
    pub fn modify_all_keys<P: Clone>(&mut self, keys: &[K], function: &dyn Fn(&V,P) -> V, parameter: P) {
        for k in keys {
            if let Some((_,v)) = self.vec.get_mut(self.hashmap[k]) {
                *v = function(&v,parameter.clone());
            };
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

















fn is_aesending<K>(data: &[K]) -> bool
where
    K: Ord,
{
    data.windows(2).all(|w| w[0] < w[1])
}

#[test]
fn check_and_time() {
    let mut x = VecHashMap::<u64,bool>::new();



    let mut rng = rand::thread_rng();
    (0..100000).for_each(|_| {
        x.insert(rand::Rng::gen(&mut rng),true);
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
        assert!(x.hashmap.get(&y.vec[i].0).copied() == me)
    });
    println!("follow time: {}ms for {} removals",runtime.elapsed().as_millis()/xlen as u128,remove.len());



}