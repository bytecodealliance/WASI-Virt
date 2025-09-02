use crate::VirtAdapter;
use crate::bindings::exports::wasi::cli::environment::Guest as Environment;
use crate::bindings::wasi::cli::environment;

#[repr(C)]
pub struct Env {
    /// Whether to fallback to the host env
    /// [byte 0]
    host_fallback: bool,
    /// Whether we are providing an allow list or a deny list
    /// on the fallback lookups
    /// [byte 1]
    host_fallback_allow: bool,
    /// How many host fields are defined in the data pointer
    /// [byte 4]
    host_field_cnt: u32,
    /// Host many host fields are defined to be allow or deny
    /// (these are concatenated at the end of the data with empty values)
    /// [byte 8]
    host_allow_or_deny_cnt: u32,
    /// Byte data of u32 byte len followed by string bytes
    /// up to the lengths previously provided.
    /// [byte 12]
    host_field_data: *const u8,
}

#[no_mangle]
pub static mut env: Env = Env {
    host_fallback: true,
    host_fallback_allow: false,
    host_field_cnt: 0,
    host_allow_or_deny_cnt: 0,
    host_field_data: 0 as *const u8,
};

fn read_data_str(offset: &mut isize) -> &'static str {
    let data: *const u8 = unsafe { env.host_field_data.offset(*offset) };
    let byte_len = unsafe { (data as *const u32).read() } as usize;
    *offset += 4;
    let data: *const u8 = unsafe { env.host_field_data.offset(*offset) };
    let str_data = unsafe { std::slice::from_raw_parts(data, byte_len) };
    *offset += byte_len as isize;
    let rem = *offset % 4;
    if rem > 0 {
        *offset += 4 - rem;
    }
    unsafe { core::str::from_utf8_unchecked(str_data) }
}

impl Environment for VirtAdapter {
    fn get_environment() -> Vec<(String, String)> {
        let mut environment = Vec::new();
        let mut data_offset: isize = 0;
        for _ in 0..unsafe { env.host_field_cnt } {
            let env_key = read_data_str(&mut data_offset);
            let env_val = read_data_str(&mut data_offset);
            environment.push((env_key.to_string(), env_val.to_string()));
        }
        let override_len = environment.len();
        // fallback ASSUMES that all data is alphabetically ordered
        if unsafe { env.host_fallback } {
            let mut allow_or_deny = Vec::new();
            for _ in 0..unsafe { env.host_allow_or_deny_cnt } {
                let allow_or_deny_key = read_data_str(&mut data_offset);
                allow_or_deny.push(allow_or_deny_key);
            }

            let is_allow_list = unsafe { env.host_fallback_allow };
            for (key, value) in environment::get_environment() {
                if environment[0..override_len]
                    .binary_search_by_key(&&key, |(s, _)| s)
                    .is_ok()
                {
                    continue;
                }
                let in_list = allow_or_deny.binary_search(&key.as_ref()).is_ok();
                if is_allow_list && in_list || !is_allow_list && !in_list {
                    environment.push((key, value));
                }
            }
        }
        environment
    }
    fn get_arguments() -> Vec<String> {
        environment::get_arguments()
    }
    fn initial_cwd() -> Option<String> {
        environment::initial_cwd()
    }
}
