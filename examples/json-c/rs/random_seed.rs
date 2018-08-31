
extern "C" {
    fn time(__timer: *mut i64) -> i64;
}

unsafe extern "C" fn get_time_seed() -> i32 {
    (time(0i32 as (*mut ::std::os::raw::c_void) as (*mut i64)) as (i32)).wrapping_mul(433494437i32)
}

pub unsafe fn json_c_get_random_seed() -> i32 {
    get_time_seed()
}
#[export_name = "json_c_get_random_seed"]
pub unsafe extern "C" fn json_c_get_random_seed_wrapper() -> i32 {
    json_c_get_random_seed()
}
