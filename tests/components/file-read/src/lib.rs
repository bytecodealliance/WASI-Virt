use std::fs;

wit_bindgen::generate!({
  path: "../../../wit",
  world: "virt-test"
});

struct VirtTestImpl;

export_virt_test!(VirtTestImpl);

impl VirtTest for VirtTestImpl {
    fn test_get_env() -> Vec<(String, String)> {
        Vec::new()
    }
    fn test_file_read(path: String) -> String {
        match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => format!("ERR: {:?}", err),
        }
    }
}
