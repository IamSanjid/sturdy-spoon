use rand::Rng;

pub fn get_elapsed_milis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("Shouldn't happen?")
        .as_millis()
}

const IS_PADDED_SIG: [u8; 3] = [69, 69, 96];
pub fn basic_pad<B: AsRef<[u8]>>(data: B, min_len: usize) -> Vec<u8> {
    let data = data.as_ref();
    let mut output = Vec::from(data);
    output.extend_from_slice(&IS_PADDED_SIG);

    let total_len = data.len() + IS_PADDED_SIG.len();
    let need_padding = total_len < min_len;
    if need_padding {
        let pad_len = min_len - total_len;
        for _ in 0..pad_len {
            output.push(rand::thread_rng().gen::<u8>());
        }
    }
    output
}

pub fn basic_unpad<'a>(data: &'a [u8]) -> &'a [u8] {
    let mut next_match_idx = 0;
    let mut current_match = 0;
    for i in 0..data.len() {
        if data[i] == IS_PADDED_SIG[next_match_idx] {
            if current_match == 0 {
                current_match = i;
            }
            next_match_idx += 1;
            if next_match_idx == IS_PADDED_SIG.len() {
                break;
            }
        } else {
            next_match_idx = 0;
            current_match = 0;
        }
    }
    if current_match == 0 {
        data
    } else {
        data.split_at(current_match).0
    }
}

/*pub fn basic_xor_enc<B: AsRef<[u8]>>(data: B, key: B) -> Vec<u8> {
    let data = data.as_ref();
    let key = key.as_ref();
    let key_len = key.len();
    let data_len = data.len();

    if data_len < key_len {
        return data.to_vec();
    }

    let mut output = Vec::with_capacity(data_len);
    let mut j = 0;

    for i in 0..data_len {
        output.push(data[i] ^ key[j]);
        j = (j + 1) % key_len;
    }

    output
}*/