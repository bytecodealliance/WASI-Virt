use std::env;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "virt-test",
    generate_all
});

struct VirtTestComponent;

impl Guest for VirtTestComponent {
    fn test_get_env() -> Vec<(String, String)> {
        env::vars().collect()
    }
    fn test_file_read(_path: String) -> String {
        unimplemented!();
    }
    fn test_stdio() -> () {
        unimplemented!();
    }
}

export!(VirtTestComponent);
