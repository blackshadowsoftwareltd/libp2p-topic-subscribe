pub fn type_of2<T>(v: T) -> (&'static str, T) {
    (std::any::type_name::<T>(), v)
}

#[macro_export]
macro_rules! type_of {
    // NOTE: We cannot use `concat!` to make a static string as a format argument
    // of `eprintln!` because `file!` could contain a `{` or
    // `$val` expression could be a block (`{ .. }`), in which case the `eprintln!`
    // will be malformed.
    () => {
        eprintln!("[{}:{}]", file!(), line!());
    };
    ($val:expr $(,)?) => {
        // Use of `match` here is intentional because it affects the lifetimes
        // of temporaries - https://stackoverflow.com/a/48732525/1063961
        match $val {
            tmp => {
                let (type_,tmp) = $crate::type_of2(tmp);
                eprintln!("[{}:{}] {}: {}",
                    file!(), line!(), stringify!($val), type_);
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::type_of!($val)),+,)
    };
}
