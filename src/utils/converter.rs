pub fn id_to_key(id: i64) -> String {
    let chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut key = String::new();
    let mut id = id;
    while id > 0 {
        key.push(chars.chars().nth(id as usize % chars.len()).unwrap());
        id /= chars.len() as i64;
    }
    key.chars().rev().collect()
}

pub fn key_to_id(key: &str) -> i64 {
    let chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut id = 0;
    for c in key.chars() {
        id = id * chars.len() as i64 + chars.find(c).unwrap() as i64;
    }
    id
}

pub fn split_short_key(short_key: &str) -> (String, String) {
    let front_random_key = short_key[..2].to_string();
    let back_random_key = short_key[short_key.len() - 2..].to_string();
    let random_key = &(front_random_key + &back_random_key);
    let unique_key = short_key[2..short_key.len() - 2].to_string();
    let url_id = key_to_id(&unique_key);
    (url_id.to_string(), random_key.to_string())
}
