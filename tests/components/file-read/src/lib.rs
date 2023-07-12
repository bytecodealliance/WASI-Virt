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
    fn test_file_read(path: String) -> Option<String> {
        match fs::read_to_string(path) {
            Ok(source) => Some(source),
            Err(err) => Some(format!("ERR: {:?}", err)),
        }
    }
}
