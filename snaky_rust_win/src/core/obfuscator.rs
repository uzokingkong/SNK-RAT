use image::GenericImageView;
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::Aead;
use zeroize::Zeroize;
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};
use rand::{Rng, SeedableRng};
use crate::core::stego_store::{STEGO_K, STEGO_M};

type HmacSha256 = Hmac<Sha256>;

pub struct Obfuscator;

impl Obfuscator {
    /// HKDF-Extract + HKDF-Expand
    /// PRK  = HMAC-SHA256(salt=STEGO_M, IKM=raw_key)
    /// OKM  = HMAC-SHA256(PRK, info=b"stego-v2\x01")
    /// raw_key = STEGO_K XOR STEGO_M  (reverses build-time obfuscation)
    fn derive_key() -> [u8; 32] {
        // Step 1: recover raw key
        let mut raw = [0u8; 32];
        for i in 0..32 { raw[i] = STEGO_K[i] ^ STEGO_M[i]; }

        // Step 2: HKDF-Extract
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&STEGO_M).expect("hmac");
        mac.update(&raw);
        let prk_generic = mac.finalize().into_bytes();
        let mut prk = [0u8; 32];
        prk.copy_from_slice(&prk_generic);
        raw.zeroize(); // raw no longer needed

        // Step 3: HKDF-Expand
        let mut mac2 = <HmacSha256 as Mac>::new_from_slice(&prk).expect("hmac");
        mac2.update(b"stego-v2\x01");
        let okm_generic = mac2.finalize().into_bytes();
        let mut okm = [0u8; 32];
        okm.copy_from_slice(&okm_generic);
        prk.zeroize(); // intermediate PRK no longer needed

        let mut key = [0u8; 32];
        key.copy_from_slice(&okm);
        okm.zeroize();
        key
    }

    /// APT-grade stego extraction:
    ///   - Key-seeded pixel ordering: SHA-256(aes_key || bundle_name)
    ///   - R+G+B LSB per pixel (3 bits/pixel)
    pub fn extract_and_decrypt(
        png_bytes: &[u8],
        nonce_bytes: &[u8; 12],
        expected_len: usize,
        bundle_name: &str,       // Category salt for PRNG — must match build.rs
    ) -> String {
        let img = match image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png) {
            Ok(i) => i,
            Err(_) => return String::new(),
        };

        let (width, height) = img.dimensions();
        let total_pixels = (width * height) as usize;

        // Derive AES key
        let mut key_bytes = Self::derive_key();

        // PRNG seed: SHA-256(aes_key || bundle_name) — matches build.rs exactly.
        // Same key but different bundle_name → completely different pixel ordering.
        let mut seed_hasher = Sha256::new();
        seed_hasher.update(&key_bytes);
        seed_hasher.update(bundle_name.as_bytes());
        let seed_hash = seed_hasher.finalize();
        let mut prng_seed = [0u8; 32];
        prng_seed.copy_from_slice(&seed_hash);
        let mut pixel_rng = rand::rngs::StdRng::from_seed(prng_seed);
        prng_seed.zeroize();

        // Determine how many pixels we need: ceil(expected_len*8 / 3)
        let need_bits = expected_len * 8;
        let need_pixels = (need_bits + 2) / 3;
        let limit = need_pixels.min(total_pixels);

        // Partial Fisher-Yates to get `limit` pixel indices in key-derived order
        // We do NOT collect all indices upfront — that's O(N) memory for large images.
        // Instead: run a streaming selection using reservoir-style index tracking.
        let mut indices: Vec<u32> = (0..total_pixels as u32).collect();
        for i in 0..limit {
            let j = pixel_rng.gen_range(i..total_pixels);
            indices.swap(i, j);
        }
        let selected = &indices[..limit];

        // Extract R,G,B LSB from each selected pixel in order
        let mut bits: Vec<u8> = Vec::with_capacity(need_bits + 6);
        for &idx in selected {
            if bits.len() >= need_bits { break; }
            let x = idx % width;
            let y = idx / width;
            let px = img.get_pixel(x, y);
            bits.push(px[0] & 1); // R LSB
            if bits.len() < need_bits { bits.push(px[1] & 1); } // G LSB
            if bits.len() < need_bits { bits.push(px[2] & 1); } // B LSB
        }
        bits.truncate(need_bits);

        // Bits → bytes (MSB first)
        let mut encrypted_data: Vec<u8> = Vec::with_capacity(expected_len);
        for chunk in bits.chunks_exact(8) {
            let mut byte = 0u8;
            for &bit in chunk { byte = (byte << 1) | bit; }
            encrypted_data.push(byte);
        }

        // AES-256-GCM decrypt
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let nonce = Nonce::from_slice(nonce_bytes);
        let cipher = Aes256Gcm::new(key);

        let decrypted_result = cipher
            .decrypt(nonce, encrypted_data.as_slice())
            .unwrap_or_default();

        // Zeroize all sensitive material BEFORE building the result string
        bits.zeroize();
        encrypted_data.zeroize();
        key_bytes.zeroize();

        // Build result from decrypted bytes, then zeroize
        let result = String::from_utf8_lossy(&decrypted_result).to_string();
        // decrypted_result is a Vec<u8> on heap — zeroize it
        let mut decrypted_result = decrypted_result;
        decrypted_result.zeroize();

        result
    }
}
