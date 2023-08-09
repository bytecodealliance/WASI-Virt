use std::env;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "virt-test",
    exports: {
        world: VirtTestComponent
    },
});

struct VirtTestComponent;

impl VirtTest for VirtTestComponent {
    fn test_get_env() -> Vec<(String, String)> {
        env::vars().collect()
    }
    fn test_file_read(_path: String) -> String {
        unimplemented!();
    }
}
