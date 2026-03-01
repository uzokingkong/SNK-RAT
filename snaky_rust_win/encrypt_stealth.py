
import os

def encrypt_xor(input_path, output_path, key):
    if not os.path.exists(input_path):
        print(f"Error: {input_path} not found.")
        return False
    
    with open(input_path, "rb") as f:
        data = bytearray(f.read())
    
    for i in range(len(data)):
        data[i] ^= key
        
    with open(output_path, "wb") as f:
        f.write(data)
    
    print(f"Successfully encrypted {input_path} to {output_path}")
    return True

if __name__ == "__main__":
    encrypt_xor("stealth.dll", "stealth.bin", 0xAA)
