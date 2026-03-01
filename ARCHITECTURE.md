# Snaky Framework - Low-Level Implementation Analysis

This document provides a detailed technical analysis of the Snaky C2 Framework based on the actual Rust source code. It covers the specialized mechanisms for data protection, memory management, and stealth execution.

---

## Data Protection and Obfuscation

### Polymorphic Steganographic Storage (StegoStore & Obfuscator)
The framework does not store sensitive strings (C2 URLs, API names, registry keys) in plaintext. Instead, it uses a multi-layered obfuscation system:
1.  **Steganographic Embedding**: Encrypted data bundles are embedded within the Least Significant Bits (LSB) of junk PNG images (`PNG_CORE`, `PNG_FS`, etc.).
2.  **Key Derivation (HKDF)**: The `Obfuscator::derive_key` function implements an APT-grade key derivation process. It reverses a build-time XOR between `STEGO_K` and `STEGO_M`, then applies HKDF-Extract and HKDF-Expand (HMAC-SHA256) to derive a unique 32-byte AES-256-GCM key.
3.  **Pixel-Seeded Extraction**: Pixels are not read linearly. The `extract_and_decrypt` function uses a PRNG seeded with `SHA-256(aes_key || bundle_name)` to perform a partial Fisher-Yates shuffle on pixel indices. This ensures that the data extraction pattern is unique for every category and build.
4.  **Memory Zeroization**: The `Obfuscator` utilizes the `zeroize` crate to securely wipe sensitive keys, nonces, and decrypted buffers from memory immediately after use, minimizing the window for memory forensics.

---

## Infrastructure and Communication

### Encrypted C2 Polling (HttpClient)
The `HttpClient` module manages communication through the Cloudflare Worker proxy using an authenticated AES-GCM tunnel:
-  **Challenge-Response Authentication**: The `perform_challenge_response` flow handles target registration. It signs a server-provided challenge using a shared secret via HMAC-SHA256 to obtain a temporary session JWT.
-  **Authenticated Tunneling**: Every request (`POST /poll`, `POST /result`) is encrypted with AES-256-GCM. The field names themselves (e.g., `J_TM` for method, `J_EP` for payload) are resolved at runtime from the `StegoStore`, leaving no identifiable JSON keys in the binary.
-  **Anti-Replay**: Each request includes a high-entropy nonce (`J_NC`), which the Cloudflare Worker validates and caches to prevent packet replaying.

---

## Stealth Execution Engine

### Runtime PE Reflection (StealthEngine)
The Nim-based stealth engine (`stealth.bin`) is never written to disk. The Rust loader implements a custom PE (Portable Executable) reflective mapper:
1.  **PEB Walking**: The `get_kernel32_base` function uses inline assembly (`mov rax, gs:[0x60]`) to access the Process Environment Block (PEB). it manually traverses the `InLoadOrderModuleList` to find `kernel32.dll` and resolve core memory management APIs without using the monitored Import Address Table (IAT).
2.  **Manual Mapping**: The `load_image` function decrypts the embedded engine, allocates a private memory region via `VirtualAlloc`, and manually handles headers, section copying, base relocations, and IAT resolution for the engine.
3.  **Dynamic Export Resolution**: Once mapped, the loader resolves internal Nim exports (e.g., `StealthKCTInject`, `StealthHollowProcess`) by manually parsing the engine's export directory in memory.

### Advanced Injection Techniques
-  **Indirect Syscalls (Halo's Gate)**: The engine avoids user-mode hooks by resolving syscall numbers directly from the `ntdll.dll` export directory on the target and executing the `syscall` instruction from a clean memory location.
-  **KCT (Kernel Callback Table) Hijacking**: The `kct_auto_inject` method targets legitimate GUI processes (like `explorer.exe`). It modifies the `KernelCallbackTable` within the target's process memory to redirect standard window message handling to malicious shellcode, ensuring execution occurs under a trusted process context without creating new threads.
-  **Process Ghosting/Hollowing**: Specialized routines allow for spawning processes in a suspended state and replacing their memory content or using temporary file handles to execute code without a backing file on disk.

---

## Security Summary
By combining HKDF-based steganography, manual PE reflection via PEB walking, and indirect kernel syscalls, the Snaky Framework maintains a zero-file footprint and bypasses modern EDR monitoring that relies on API hooking and static signature analysis.
