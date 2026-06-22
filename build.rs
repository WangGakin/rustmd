fn main() {
    #[cfg(windows)]
    embed_resource::compile("res/icon.rc", std::iter::empty::<&str>());
}
