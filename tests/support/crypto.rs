// Independent test-only implementations of the v2 keyword ciphers. These do
// not call mdictlib internals, so whole-file encryption fixtures can catch
// wiring, byte-order, and section-boundary mistakes in the reader.

const RIPEMD128_R: [usize; 64] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9, 5,
    2, 14, 11, 8, 3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12, 1, 9, 11, 10, 0, 8, 12, 4,
    13, 3, 7, 15, 14, 5, 6, 2,
];

const RIPEMD128_RP: [usize; 64] = [
    5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12, 6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8, 12,
    4, 9, 1, 2, 15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13, 8, 6, 4, 1, 3, 11, 15, 0, 5,
    12, 2, 13, 9, 7, 10, 14,
];

const RIPEMD128_S: [u32; 64] = [
    11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8, 7, 6, 8, 13, 11, 9, 7, 15, 7, 12, 15,
    9, 11, 7, 13, 12, 11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5, 11, 12, 14, 15, 14,
    15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12,
];

const RIPEMD128_SP: [u32; 64] = [
    8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6, 9, 13, 15, 7, 12, 8, 9, 11, 7, 7, 12,
    7, 6, 15, 13, 11, 9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5, 15, 5, 8, 11, 14, 14,
    6, 14, 6, 9, 12, 9, 12, 5, 15, 8,
];

pub fn ripemd128(bytes: &[u8]) -> [u8; 16] {
    let mut padded = bytes.to_vec();
    let bit_len = u64::try_from(bytes.len()).unwrap().wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());

    let mut h0 = 0x6745_2301u32;
    let mut h1 = 0xefcd_ab89u32;
    let mut h2 = 0x98ba_dcfeu32;
    let mut h3 = 0x1032_5476u32;

    for chunk in padded.chunks_exact(64) {
        let mut words = [0u32; 16];
        for (index, word) in words.iter_mut().enumerate() {
            let start = index * 4;
            *word = u32::from_le_bytes(chunk[start..start + 4].try_into().unwrap());
        }

        let (mut a, mut b, mut c, mut d) = (h0, h1, h2, h3);
        let (mut aa, mut bb, mut cc, mut dd) = (h0, h1, h2, h3);
        for index in 0..64 {
            let next = a
                .wrapping_add(f(index, b, c, d))
                .wrapping_add(words[RIPEMD128_R[index]])
                .wrapping_add(k(index))
                .rotate_left(RIPEMD128_S[index]);
            a = d;
            d = c;
            c = b;
            b = next;

            let next = aa
                .wrapping_add(fp(index, bb, cc, dd))
                .wrapping_add(words[RIPEMD128_RP[index]])
                .wrapping_add(kp(index))
                .rotate_left(RIPEMD128_SP[index]);
            aa = dd;
            dd = cc;
            cc = bb;
            bb = next;
        }

        let next = h1.wrapping_add(c).wrapping_add(dd);
        h1 = h2.wrapping_add(d).wrapping_add(aa);
        h2 = h3.wrapping_add(a).wrapping_add(bb);
        h3 = h0.wrapping_add(b).wrapping_add(cc);
        h0 = next;
    }

    let mut output = [0u8; 16];
    for (index, word) in [h0, h1, h2, h3].into_iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    output
}

pub fn encrypt_keyword_index(checksum: u32, data: &mut [u8]) {
    let mut key_material = checksum.to_be_bytes().to_vec();
    key_material.extend_from_slice(&[0x95, 0x36, 0x00, 0x00]);
    let key = ripemd128(&key_material);

    let mut previous_cipher = 0x36u8;
    for (index, byte) in data.iter_mut().enumerate() {
        let plain = *byte;
        let cipher =
            (plain ^ (index as u8) ^ key[index % key.len()] ^ previous_cipher).rotate_left(4);
        *byte = cipher;
        previous_cipher = cipher;
    }
}

pub fn encrypt_keyword_header(data: &mut [u8], reg_code_hex: &str, user_id: &str) {
    let mut encrypted_hash = decode_hex_16(reg_code_hex);
    let user_hash = ripemd128(user_id.as_bytes());
    salsa20_8_xor(&mut encrypted_hash, &user_hash);
    salsa20_8_xor(data, &encrypted_hash);
}

fn decode_hex_16(text: &str) -> [u8; 16] {
    assert_eq!(text.len(), 32);
    let mut output = [0u8; 16];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    output
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("invalid fixture hex digit"),
    }
}

fn salsa20_8_xor(data: &mut [u8], key: &[u8; 16]) {
    for (counter, chunk) in data.chunks_mut(64).enumerate() {
        let keystream = salsa20_8_block(key, u64::try_from(counter).unwrap());
        for (destination, source) in chunk.iter_mut().zip(keystream) {
            *destination ^= source;
        }
    }
}

fn salsa20_8_block(key: &[u8; 16], counter: u64) -> [u8; 64] {
    let constants = *b"expand 16-byte k";
    let key_words = [
        u32::from_le_bytes(key[0..4].try_into().unwrap()),
        u32::from_le_bytes(key[4..8].try_into().unwrap()),
        u32::from_le_bytes(key[8..12].try_into().unwrap()),
        u32::from_le_bytes(key[12..16].try_into().unwrap()),
    ];
    let state = [
        u32::from_le_bytes(constants[0..4].try_into().unwrap()),
        key_words[0],
        key_words[1],
        key_words[2],
        key_words[3],
        u32::from_le_bytes(constants[4..8].try_into().unwrap()),
        0,
        0,
        counter as u32,
        (counter >> 32) as u32,
        u32::from_le_bytes(constants[8..12].try_into().unwrap()),
        key_words[0],
        key_words[1],
        key_words[2],
        key_words[3],
        u32::from_le_bytes(constants[12..16].try_into().unwrap()),
    ];

    let mut working = state;
    for _ in 0..4 {
        working = row_round(column_round(working));
    }
    for (word, original) in working.iter_mut().zip(state) {
        *word = word.wrapping_add(original);
    }

    let mut output = [0u8; 64];
    for (index, word) in working.into_iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    output
}

fn column_round(mut state: [u32; 16]) -> [u32; 16] {
    quarter_round(&mut state, 0, 4, 8, 12);
    quarter_round(&mut state, 5, 9, 13, 1);
    quarter_round(&mut state, 10, 14, 2, 6);
    quarter_round(&mut state, 15, 3, 7, 11);
    state
}

fn row_round(mut state: [u32; 16]) -> [u32; 16] {
    quarter_round(&mut state, 0, 1, 2, 3);
    quarter_round(&mut state, 5, 6, 7, 4);
    quarter_round(&mut state, 10, 11, 8, 9);
    quarter_round(&mut state, 15, 12, 13, 14);
    state
}

fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[b] ^= state[a].wrapping_add(state[d]).rotate_left(7);
    state[c] ^= state[b].wrapping_add(state[a]).rotate_left(9);
    state[d] ^= state[c].wrapping_add(state[b]).rotate_left(13);
    state[a] ^= state[d].wrapping_add(state[c]).rotate_left(18);
}

fn f(index: usize, x: u32, y: u32, z: u32) -> u32 {
    match index {
        0..=15 => x ^ y ^ z,
        16..=31 => (x & y) | (!x & z),
        32..=47 => (x | !y) ^ z,
        _ => (x & z) | (y & !z),
    }
}

fn fp(index: usize, x: u32, y: u32, z: u32) -> u32 {
    match index {
        0..=15 => (x & z) | (y & !z),
        16..=31 => (x | !y) ^ z,
        32..=47 => (x & y) | (!x & z),
        _ => x ^ y ^ z,
    }
}

fn k(index: usize) -> u32 {
    match index {
        0..=15 => 0,
        16..=31 => 0x5a82_7999,
        32..=47 => 0x6ed9_eba1,
        _ => 0x8f1b_bcdc,
    }
}

fn kp(index: usize) -> u32 {
    match index {
        0..=15 => 0x50a2_8be6,
        16..=31 => 0x5c4d_d124,
        32..=47 => 0x6d70_3ef3,
        _ => 0,
    }
}
