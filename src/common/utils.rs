pub fn get_elapsed_milis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("Shouldn't happen?")
        .as_millis()
}