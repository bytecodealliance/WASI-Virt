use std::env;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "virt-test"
});

struct VirtTestImpl;

export_virt_test!(VirtTestImpl);

impl VirtTest for VirtTestImpl {
    fn test_get_env() -> Vec<(String, String)> {
        env::vars().collect()
    }
    fn test_file_read(_path: String) -> String {
        unimplemented!();
    }
}
