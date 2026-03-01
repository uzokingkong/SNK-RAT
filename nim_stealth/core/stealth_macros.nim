import macros

# Compile-time XOR string encryption
# This ensures plain text strings are NOT stored in the binary
macro enc*(s: static string): untyped =
    var encrypted: seq[byte] = @[]
    let key: byte = 0x42 # Simple static key for example
    
    for i in 0..<s.len:
        encrypted.add(s[i].byte xor key)
    encrypted.add(0.byte) # Null terminator
    
    result = newTree(nnkBracket)
    for b in encrypted:
        result.add(newLit(b))

# Runtime XOR decryption helper
proc dec*(bytes: openArray[byte]): string =
    let key: byte = 0x42
    result = newString(bytes.len - 1)
    for i in 0..<bytes.len - 1:
        result[i] = (bytes[i] xor key).char

# Debugging version of decrypt that logs to console
proc dec_debug*(bytes: openArray[byte], label: string): string =
    result = dec(bytes)
    # echo "  [DEBUG] Decrypting string for '", label, "' -> Result: '", result, "'"
