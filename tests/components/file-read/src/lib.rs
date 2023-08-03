use std::fs;

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
        Vec::new()
    }
    fn test_file_read(path: String) -> String {
        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(err) => {
                return format!("ERR: reading metadata {path}\n{:?}", err);
            }
        };
        if meta.is_file() {
            match fs::read_to_string(&path) {
                Ok(source) => source,
                Err(err) => format!("ERR: {:?}", err),
            }
        } else if meta.is_dir() {
            let dir = match fs::read_dir(&path) {
                Ok(dir) => dir,
                Err(err) => {
                    return format!("ERR: reading dir {path}\n{:?}", err);
                }
            };
            let mut files = String::new();
            for file in dir {
                let file = match file {
                    Ok(file) => file,
                    Err(err) => {
                        return format!("ERR: reading dir entry\n{:?}", err);
                    }
                };
                files.push_str(match file.file_name().to_str() {
                    Some(name) => name,
                    None => {
                        return format!("ERR: invalid filename string '{:?}'", file.file_name());
                    }
                });
            }
            files
        } else {
            "ERR: Not a file or dir".into()
        }
    }
}
