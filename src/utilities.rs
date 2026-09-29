use rand::distr::Alphanumeric;
use rand::{rng, RngExt};

pub(crate) fn generate_random_string(length: usize) -> String {
    rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}
