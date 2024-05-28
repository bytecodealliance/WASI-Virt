use rand::prelude::*;
use std::env;
use std::fs;
use std::time::SystemTime;

extern crate rand;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "virt-test",
});

struct VirtTestComponent;

impl Guest for VirtTestComponent {
    fn test_get_env() -> Vec<(String, String)> {
        unreachable!();
    }
    fn test_file_read(path: String) -> String {
        let vars: Vec<(String, String)> = env::vars().collect();
        let mut rng = rand::thread_rng();
        println!("({:?}) TEST STDOUT - {:?}", SystemTime::now(), vars);
        eprintln!(
            "({:?}) TEST STDERR - {}",
            SystemTime::now(),
            rng.gen::<u32>()
        );
        fs::read_to_string(&path).unwrap_or_else(|e| format!("ERR: {:?}", e))
    }
    fn test_stdio() -> () {
        unimplemented!();
    }
}

export!(VirtTestComponent);
