wit_bindgen::generate!({
    path: "../../../wit/0_2_3",
    world: "virt-test",
    generate_all
});

struct VirtTestComponent;

impl Guest for VirtTestComponent {
    fn test_get_env() -> Vec<(String, String)> {
        unimplemented!();
    }
    fn test_get_config() -> Vec<(String, String)> {
        unimplemented!();
    }
    fn test_file_read(_path: String) -> String {
        unimplemented!();
    }
    fn test_stdio() -> () {
        println!("Hello world");
    }
}

export!(VirtTestComponent);
