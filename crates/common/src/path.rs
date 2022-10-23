use lazy_static::lazy_static;
use regex::Regex;

pub fn convert_path_to_posix(path: &str) -> String {
    lazy_static! {
        static ref PATH_REPLACEMENT_REGEX: Regex = Regex::new(r":\\|\\").unwrap();
    }

    PATH_REPLACEMENT_REGEX.replace_all(path, "/").to_string()
}
