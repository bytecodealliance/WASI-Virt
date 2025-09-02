use std::env;
use std::fs;
use std::time::SystemTime;

use rand::prelude::*;

wit_bindgen::generate!({
    path: "../../../wit/0_2_1",
    world: "virt-test",
    generate_all
});

struct VirtTestComponent;

impl Guest for VirtTestComponent {
    fn test_get_env() -> Vec<(String, String)> {
        unreachable!();
    }
    fn test_get_config() -> Vec<(String, String)> {
        unreachable!();
    }
    fn test_file_read(path: String) -> String {
        let vars: Vec<(String, String)> = env::vars().collect();
        let mut rng = rand::rng();
        println!("({:?}) TEST STDOUT - {:?}", SystemTime::now(), vars);
        eprintln!(
            "({:?}) TEST STDERR - {}",
            SystemTime::now(),
            rng.random::<u32>()
        );
        fs::read_to_string(&path).unwrap_or_else(|e| format!("ERR: {:?}", e))
    }
    fn test_stdio() -> () {
        unimplemented!();
    }
}

export!(VirtTestComponent);
